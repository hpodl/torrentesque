use std::cmp::min;
use std::ops::Deref;
use std::str;

use bit_vec::BitVec;
use serde::{ser::SerializeStruct, Deserialize, Deserializer, Serialize, Serializer};
use tokio::fs::{read, File, OpenOptions};
use tokio::io::{self, AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::RwLock;

use std::fs::File as StdFile;

/// Handles the logic of dividing the file into packets, writing and reading them.
pub struct TorrentFile {
    path: String,
    torrent_size: usize,
    packet_size: usize,
    packet_count: usize,
    packet_availability: RwLock<BitVec>,
    file: RwLock<File>,
}

/// Returns ceil(a/b)
///
/// Currently stable Rust offers no straightforward integer ceil division
fn div_usize_ceil(a: usize, b: usize) -> usize {
    let floor = a / b;
    if floor * b < a {
        floor + 1
    } else {
        floor
    }
}

impl TorrentFile {
    pub fn new(path: &str, torrent_size: usize, packet_size: usize) -> io::Result<Self> {
        // std::fs::File
        let file: StdFile = StdFile::options()
            .write(true)
            .read(true)
            .create(true)
            .truncate(true)
            .open(path)?;

        // tokio::fs::File
        let file = RwLock::new(File::from_std(file));

        // currently stable Rust offers no straightforward integer ceil division
        let packet_count = div_usize_ceil(torrent_size, packet_size);

        let path = path.to_owned();
        let mut packet_availability = BitVec::new();
        packet_availability.grow(packet_count, false);
        let packet_availability = RwLock::new(packet_availability);

        Ok(Self {
            path,
            torrent_size,
            packet_size,
            packet_count,
            packet_availability,
            file,
        })
    }

    /// Saves torrent metadata and download progress to a file named `[torrent_name].progress`
    pub async fn save_progress_to_file(&self) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(format!("{}.progress", self.path))
            .await?;

        file.write_all(&serde_json::to_vec(&self)?).await
    }

    /// Creates the struct based on metadata saved to a progress file
    pub async fn from_progress_file(path: &str) -> io::Result<Self> {
        let file_content = read(path).await?;

        // If returned directly, it'll have serde_json::Result type, which is incompatible with return type
        let deserialized = serde_json::from_slice(&file_content)?;
        Ok(deserialized)
    }

    /// Creates the struct assuming the file pointed to by `path` is correct and downloaded wholly
    ///
    /// `from_progress_file` should be preferred over this one
    pub fn from_complete(path: &str, packet_size: usize) -> io::Result<Self> {
        let file = StdFile::options().read(true).truncate(false).open(path)?;
        let torrent_size = file.metadata()?.len() as usize;

        let packet_count = div_usize_ceil(torrent_size, packet_size);
        let mut packet_availability = BitVec::new();
        packet_availability.grow(packet_count, true);
        let packet_availability = RwLock::new(packet_availability);

        let file = RwLock::new(File::from_std(file));
        Ok(Self {
            path: path.to_owned(),
            torrent_size,
            packet_size,
            packet_count,
            packet_availability,
            file,
        })
    }

    pub fn packet_count(&self) -> usize {
        self.packet_count
    }

    /// Acquires internal RwLock and returns BitVec representing which packets are available and which are not
    pub async fn read_packet_availability(&self) -> BitVec {
        self.packet_availability.read().await.clone()
    }

    pub fn packet_size(&self) -> usize {
        self.packet_size
    }

    /// Reads packets [start; start + count] from a file
    pub async fn read_packets(&self, start: usize, count: usize) -> io::Result<Vec<u8>> {
        if start + count > self.packet_count {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Packet out of bounds".to_owned(),
            ));
        }
        let all_available = {
            let packet_availability = self.packet_availability.read().await;

            packet_availability
                .iter()
                .skip(start)
                .take(count)
                .all(|bit| bit == true)
        };

        if !all_available {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Not all requested packets are available.".to_owned(),
            ));
        }

        // Makes the buffer smaller when the last packet is of size < `self.packet_size`
        let bytes_to_read = min(
            count * self.packet_size,
            self.torrent_size - start * self.packet_size,
        );
        let mut buf = vec![0u8; bytes_to_read];

        // For now, has to be a mutable `RwLockWriteGuard` as it actually modifies internal cursor
        let mut reader = self.file.write().await;
        reader
            .seek(io::SeekFrom::Start((start * self.packet_size) as u64))
            .await?;
        reader.read_exact(&mut buf).await?;

        Ok(buf.to_owned())
    }

    pub async fn write_packets(&self, start: usize, data: &[u8]) -> io::Result<()> {
        let mut writer = self.file.write().await;
        writer
            .seek(io::SeekFrom::Start((start * self.packet_size) as u64))
            .await?;
        writer.write_all(data).await?;
        writer.flush().await?;

        let mut availability_lock = self.packet_availability.write().await;
        for i in start..(start + div_usize_ceil(data.len(), self.packet_size)) {
            availability_lock.set(i, true);
        }

        writer.flush().await?;
        Ok(())
    }
}

impl Serialize for TorrentFile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("FileHandler", 5)?;
        state.serialize_field("path", &self.path)?;
        state.serialize_field("torrent_size", &self.torrent_size)?;
        state.serialize_field("packet_size", &self.packet_size)?;
        state.serialize_field("packet_count", &self.packet_count)?;
        state.serialize_field(
            "packet_availability",
            &self.packet_availability.try_read().unwrap().deref(),
        )?;
        state.end()
    }
}
impl<'de> Deserialize<'de> for TorrentFile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "path",
            "torrent_size",
            "packet_size",
            "packet_count",
            "packet_availability",
        ];
        deserializer.deserialize_struct("FileHandler", FIELDS, FileHandlerVisitor)
    }
}

struct FileHandlerVisitor;

impl<'de> serde::de::Visitor<'de> for FileHandlerVisitor {
    type Value = TorrentFile;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("struct FileHandler")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let path = seq
            .next_element()?
            .ok_or_else(|| serde::de::Error::invalid_length(0, &self))?;
        let torrent_size = seq
            .next_element()?
            .ok_or_else(|| serde::de::Error::invalid_length(1, &self))?;
        let packet_size = seq
            .next_element()?
            .ok_or_else(|| serde::de::Error::invalid_length(2, &self))?;
        let packet_count = seq
            .next_element()?
            .ok_or_else(|| serde::de::Error::invalid_length(3, &self))?;
        let packet_availability = RwLock::new(
            seq.next_element()?
                .ok_or_else(|| serde::de::Error::invalid_length(4, &self))?,
        );

        let file = StdFile::options()
            .write(true)
            .read(true)
            .create(false)
            .truncate(false)
            .open(&path)
            .unwrap();

        let file = RwLock::new(File::from_std(file));

        Ok(TorrentFile {
            file,
            path,
            torrent_size,
            packet_size,
            packet_count,
            packet_availability,
        })
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut path = None;
        let mut torrent_size = None;
        let mut packet_size = None;
        let mut packet_count = None;
        let mut packet_availability = None;

        while let Some(key) = map.next_key()? {
            match key {
                "path" => {
                    if path.is_some() {
                        return Err(serde::de::Error::duplicate_field("path"));
                    }
                    path = Some(map.next_value()?);
                }
                "torrent_size" => {
                    if torrent_size.is_some() {
                        return Err(serde::de::Error::duplicate_field("torrent_size"));
                    }
                    torrent_size = Some(map.next_value()?);
                }
                "packet_size" => {
                    if packet_size.is_some() {
                        return Err(serde::de::Error::duplicate_field("packet_size"));
                    }
                    packet_size = Some(map.next_value()?);
                }
                "packet_count" => {
                    if packet_count.is_some() {
                        return Err(serde::de::Error::duplicate_field("packet_count"));
                    }
                    packet_count = Some(map.next_value()?);
                }
                "packet_availability" => {
                    if packet_availability.is_some() {
                        return Err(serde::de::Error::duplicate_field("packet_availability"));
                    }
                    packet_availability = Some(RwLock::new(map.next_value()?));
                }
                _ => {
                    let _ = map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
        }

        let path = path.ok_or_else(|| serde::de::Error::missing_field("path"))?;
        let torrent_size =
            torrent_size.ok_or_else(|| serde::de::Error::missing_field("torrent_size"))?;
        let packet_size =
            packet_size.ok_or_else(|| serde::de::Error::missing_field("packet_size"))?;
        let packet_count =
            packet_count.ok_or_else(|| serde::de::Error::missing_field("packet_count"))?;
        let packet_availability = packet_availability
            .ok_or_else(|| serde::de::Error::missing_field("packet_availability"))?;

        let file = StdFile::options()
            .write(true)
            .read(true)
            .create(false)
            .truncate(false)
            .open(&path)
            .unwrap();

        let file = RwLock::new(File::from_std(file));

        Ok(TorrentFile {
            file,
            path,
            torrent_size,
            packet_size,
            packet_count,
            packet_availability,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)] // to allow structs' original case in test names

    use super::*;
    use std::io::Read;

    #[test]
    fn div_usize_ceil_same_as_floor() {
        assert_eq!(div_usize_ceil(40, 20), 2);
        assert_eq!(div_usize_ceil(134, 20), 7);
        assert_eq!(div_usize_ceil(1, 1), 1);
        assert_eq!(div_usize_ceil(21, 1), 21);
    }

    #[test]
    fn div_usize_ceil_rounds_up() {
        assert_eq!(div_usize_ceil(40, 21), 2);
        assert_eq!(div_usize_ceil(40, 19), 3);
        assert_eq!(div_usize_ceil(17, 2), 9);
        assert_eq!(div_usize_ceil(13, 3), 5);
    }

    #[tokio::test]
    async fn FileHandler_new() {
        let filename = ".testfiles/test_file";
        let handler = TorrentFile::new(filename, 10, 1);
        assert!(handler.is_ok());
        assert_eq!(handler.unwrap().packet_count, 10);
    }

    #[tokio::test]
    async fn FileHandler_last_packet_not_whole() {
        let filename = ".testfiles/test_file";
        let handler = TorrentFile::new(filename, 10, 4).unwrap();
        assert_eq!(handler.packet_count, 3);
        assert_eq!(handler.read_packet_availability().await.len(), 3);
    }

    #[tokio::test]
    async fn FileHandler_write_packets_get_packets() {
        let filename = ".testfiles/FileHandler_write_packets";
        let packet_size = 4; // 4 bytes

        let handler = TorrentFile::new(filename, 10, packet_size).unwrap();

        handler
            .write_packets(0, "ABCDabcd".as_bytes())
            .await
            .unwrap();
        assert_eq!(
            handler.read_packets(0, 2).await.unwrap(),
            "ABCDabcd".as_bytes()
        )
    }

    #[tokio::test]
    async fn FileHandler_write_packets_actual_file() {
        let filename = ".testfiles/FileHandler_write_packets_file";
        let packet_size = 4; // 4 bytes

        let handler = TorrentFile::new(filename, 10, packet_size).unwrap();

        handler
            .write_packets(0, "ABCDabcd".as_bytes())
            .await
            .unwrap();
        let mut file = StdFile::options()
            .read(true)
            .write(false)
            .truncate(false)
            .open(filename)
            .unwrap();
        let mut buf = [0u8; 8];
        file.read_exact(&mut buf).unwrap();

        assert_eq!(buf, "ABCDabcd".as_bytes());
    }

    #[tokio::test]
    async fn FileHandler_write_packets_non_divisible() {
        let filename = ".testfiles/FileHandler_write_packets_nondiv";
        let packet_size = 4; // 4 bytes

        let handler = TorrentFile::new(filename, 10, packet_size).unwrap();

        handler
            .write_packets(0, "ABCDabcd".as_bytes())
            .await
            .unwrap();
        assert_eq!(
            handler.read_packets(0, 2).await.unwrap(),
            "ABCDabcd".as_bytes()
        );

        let mut file = StdFile::open(filename).unwrap();
        let mut buf = [0u8; 8];
        file.read_exact(&mut buf).unwrap();
    }

    #[tokio::test]
    async fn FileHandler_read_packets_out_of_bounds() {
        let filename = ".testfiles/FileHandler_write_packets_nondiv";
        let packet_size = 4; // 4 bytes

        let handler = TorrentFile::new(filename, 10, packet_size).unwrap();

        handler
            .write_packets(0, "ABCDabcd".as_bytes())
            .await
            .unwrap();
        assert!(handler.read_packets(0, 256).await.is_err(),);

        let mut file = StdFile::open(filename).unwrap();
        let mut buf = [0u8; 8];
        file.read_exact(&mut buf).unwrap();
    }

    #[tokio::test]
    async fn FileHandler_read_packets_unavailable() {
        let filename = ".testfiles/FileHandler_write_packets_nondiv";
        let packet_size = 4; // 4 bytes

        let handler = TorrentFile::new(filename, 10, packet_size).unwrap();

        handler
            .write_packets(0, "ABCDabcd".as_bytes())
            .await
            .unwrap();
        assert!(handler.read_packets(3, 4).await.is_err(),);

        let mut file = StdFile::open(filename).unwrap();
        let mut buf = [0u8; 8];
        file.read_exact(&mut buf).unwrap();
    }

    #[tokio::test]
    async fn FileHandler_get_packet_availability() {
        let filename = ".testfiles/FileHandler_get_packet_availability";
        let handler = TorrentFile::new(filename, 8, 1).unwrap();

        let mut vec = BitVec::from_bytes(&[0]);
        assert_eq!(handler.read_packet_availability().await, vec);

        handler.write_packets(0, "AA".as_bytes()).await.unwrap();
        vec.set(0, true);
        vec.set(1, true);
        assert_eq!(handler.read_packet_availability().await, vec);
    }

    #[tokio::test]
    async fn FileHandler_serde() {
        let content = "ABCDabcd".as_bytes();
        let filename = ".testfiles/FileHandler_serde";
        let handler = TorrentFile::new(filename, 8, 1).unwrap();
        handler.write_packets(0, content).await.unwrap();

        let serialized = serde_json::to_string(&handler).unwrap();
        println!("\n{:#?}\n", serialized);
        let deserialized: TorrentFile = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.read_packets(0, 8).await.unwrap(), content)
    }
}
