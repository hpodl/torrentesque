use std::cmp::min;
use std::str;

use bit_vec::BitVec;
use serde::{ser::SerializeStruct, Deserialize, Deserializer, Serialize, Serializer};
use tokio::fs::{read_to_string, File, OpenOptions};
use tokio::io::{self, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader};

use std::fs::File as StdFile;

/// Handles the logic of dividing the file into packets, writing and reading them.
pub struct FileHandler {
    path: String,
    torrent_size: usize,
    packet_size: usize,
    packet_count: usize,
    packet_availability: BitVec,
    file: StdFile,
}

/// Returns ceil(a/b)
/// 
/// Currently stable Rust offers no straightforward integer ceil division
fn div_usize_ceil(a: usize, b: usize) -> usize {
    let floor = a / b;
    if floor*b < a {
        floor + 1
    } else {
        floor
    }
}

impl FileHandler {
    pub fn new(path: &str, torrent_size: usize, packet_size: usize) -> io::Result<Self> {
        let file = StdFile::options()
            .write(true)
            .read(true)
            .create(true)
            .truncate(true)
            .open(path)?;

        // currently stable Rust offers no straightforward integer ceil division
        let packet_count = div_usize_ceil(torrent_size, packet_size);

        let path = path.to_owned();
        let mut packet_availability = BitVec::new();
        packet_availability.grow(packet_count, false);

        Ok(Self {
            path,
            torrent_size,
            packet_size,
            packet_count,
            packet_availability: packet_availability,
            file,
        })
    }

    pub async fn save_progress(&self) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(format!("{}.prog", self.path))
            .await?;

        file.write_all(
            &serde_json::to_string(&self)
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?
                .as_bytes(),
        )
        .await
    }

    pub async fn from_existing(path: &str) -> io::Result<Self> {
        let file_content = read_to_string(path).await?;
        serde_json::from_str(&file_content)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))
    }

    pub async fn get_packets(&self, start: usize, count: usize) -> io::Result<Vec<u8>> {
        // Makes the buffer smaller when the last packet is of size < `self.packet_size`
        let bytes_to_read = min(
            count * self.packet_size,
            self.torrent_size - start * self.packet_size,
        );
        let mut buf = vec![0u8; bytes_to_read];

        let mut reader = BufReader::new(File::from_std(self.file.try_clone()?));
        reader
            .seek(io::SeekFrom::Start((start * self.packet_size) as u64))
            .await?;
        reader.read_exact(&mut buf).await?;

        Ok(buf.to_owned())
    }

    pub async fn write_packets(&mut self, start: usize, data: &[u8]) -> io::Result<()> {
        let mut file_handler = File::from_std(self.file.try_clone()?);
        file_handler
            .seek(io::SeekFrom::Start((start * self.packet_size) as u64))
            .await?;
        file_handler.write_all(data).await?;
        file_handler.flush().await?;

        for i in start..(start + div_usize_ceil(data.len(), self.packet_size)) {
            self.packet_availability.set(i, true);
        }

        Ok(())
    }

    pub fn packet_count(&self) -> usize {
        self.packet_count
    }

    pub fn packet_availability(&self) -> BitVec {
        self.packet_availability.clone()
    }
}

impl Serialize for FileHandler {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("FileHandler", 5)?;
        state.serialize_field("path", &self.path)?;
        state.serialize_field("torrent_size", &self.torrent_size)?;
        state.serialize_field("packet_size", &self.packet_size)?;
        state.serialize_field("packet_count", &self.packet_count)?;
        state.serialize_field("packet_availability", &self.packet_availability)?;
        state.end()
    }
}
impl<'de> Deserialize<'de> for FileHandler {
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
    type Value = FileHandler;

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
        let packet_availability = seq
            .next_element()?
            .ok_or_else(|| serde::de::Error::invalid_length(4, &self))?;
        Ok(FileHandler {
            file: StdFile::open("").unwrap(),
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
                    packet_availability = Some(map.next_value()?);
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
            .append(true)
            .read(true)
            .create(false)
            .truncate(false)
            .open(&path)
            .unwrap();

        Ok(FileHandler {
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

    use std::io::Read;
    use super::*;
    
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
        let handler = FileHandler::new(filename, 10, 1);
        assert!(handler.is_ok());
        assert_eq!(handler.unwrap().packet_count, 10);
    }

    #[tokio::test]
    async fn FileHandler_write_packets_get_packets() {
        let filename = ".testfiles/FileHandler_write_packets";
        let packet_size = 4; // 4 bytes

        let mut handler = FileHandler::new(filename, 10, packet_size).unwrap();

        handler
            .write_packets(0, "ABCDabcd".as_bytes())
            .await
            .unwrap();
        assert_eq!(
            handler.get_packets(0, 2).await.unwrap(),
            "ABCDabcd".as_bytes()
        )
    }

    #[tokio::test]
    async fn FileHandler_write_packets_actual_file() {
        let filename = ".testfiles/FileHandler_write_packets";
        let packet_size = 4; // 4 bytes

        let mut handler = FileHandler::new(filename, 10, packet_size).unwrap();

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
        let filename = ".testfiles/FileHandler_write_packets";
        let packet_size = 4; // 4 bytes

        let mut handler = FileHandler::new(filename, 10, packet_size).unwrap();

        handler
            .write_packets(0, "ABCDabcd".as_bytes())
            .await
            .unwrap();
        assert_eq!(
            handler.get_packets(0, 2).await.unwrap(),
            "ABCDabcd".as_bytes()
        );

        let mut file = StdFile::open(filename).unwrap();
        let mut buf = [0u8; 8];
        file.read_exact(&mut buf).unwrap();
    }

    #[tokio::test]
    async fn FileHandler_get_packet_availability() {
        let filename = ".testfiles/FileHandler_get_packet_availability";
        let mut handler = FileHandler::new(filename, 8, 1).unwrap();

        let mut vec = BitVec::from_bytes(&[0]);
        assert_eq!(handler.packet_availability(), vec);

        handler.write_packets(0, "AA".as_bytes()).await.unwrap();
        vec.set(0, true);
        vec.set(1, true);
        assert_eq!(handler.packet_availability(), vec);
    }

    #[tokio::test]
    async fn FileHandler_serde() {
        let content = "ABCDabcd".as_bytes();
        let filename = ".testfiles/FileHandler_serde";
        let mut handler = FileHandler::new(filename, 8, 1).unwrap();
        handler.write_packets(0, content).await.unwrap();

        let serialized = serde_json::to_string(&handler).unwrap();
        println!("\n{:#?}\n", serialized);
        let deserialized: FileHandler = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.get_packets(0, 8).await.unwrap(), content)
    }
}
