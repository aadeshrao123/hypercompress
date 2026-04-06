use std::io::{self, Read, Write};
use thiserror::Error;

pub const MAGIC: [u8; 4] = [b'H', b'C', 0x01, 0x00];
pub const FORMAT_VERSION: u8 = 1;

#[derive(Error, Debug)]
pub enum FormatError {
    #[error("invalid magic bytes")]
    InvalidMagic,
    #[error("unsupported version: {0}")]
    UnsupportedVersion(u8),
    #[error("checksum mismatch for chunk {0}")]
    ChecksumMismatch(u32),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DataType {
    Text = 0,
    Structured = 1,
    Binary = 2,
    NumericInt = 3,
    NumericFloat = 4,
    CompressedOrRandom = 5,
    Sparse = 6,
}

impl DataType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Text),
            1 => Some(Self::Structured),
            2 => Some(Self::Binary),
            3 => Some(Self::NumericInt),
            4 => Some(Self::NumericFloat),
            5 => Some(Self::CompressedOrRandom),
            6 => Some(Self::Sparse),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TransformType {
    None = 0,
    Bwt = 1,
    Delta = 2,
    FloatSplit = 3,
    Transpose = 4,
    Rle = 5,
    BitPlane = 6,
    BwtMtf = 7,
    Prediction = 8,
    StructSplit = 9,
    Bcj = 10,
    Precomp = 11,
}

impl TransformType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Bwt),
            2 => Some(Self::Delta),
            3 => Some(Self::FloatSplit),
            4 => Some(Self::Transpose),
            5 => Some(Self::Rle),
            6 => Some(Self::BitPlane),
            7 => Some(Self::BwtMtf),
            8 => Some(Self::Prediction),
            9 => Some(Self::StructSplit),
            10 => Some(Self::Bcj),
            11 => Some(Self::Precomp),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CodecType {
    Raw = 0,
    Ans = 1,
    Lz = 2,
    LzAns = 3,
    Order1 = 4,
    LzOptimal = 5,
    Lzma = 6,
    Zstd = 7,
}

impl CodecType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Raw),
            1 => Some(Self::Ans),
            2 => Some(Self::Lz),
            3 => Some(Self::LzAns),
            4 => Some(Self::Order1),
            5 => Some(Self::LzOptimal),
            6 => Some(Self::Lzma),
            7 => Some(Self::Zstd),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileHeader {
    pub version: u8,
    pub flags: u16,
    pub original_size: u64,
    pub chunk_count: u32,
    pub segment_map_offset: u64,
}

impl FileHeader {
    pub const SIZE: usize = 4 + 1 + 2 + 8 + 4 + 8; // 27 bytes

    pub fn write_to<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(&MAGIC)?;
        w.write_all(&[self.version])?;
        w.write_all(&self.flags.to_le_bytes())?;
        w.write_all(&self.original_size.to_le_bytes())?;
        w.write_all(&self.chunk_count.to_le_bytes())?;
        w.write_all(&self.segment_map_offset.to_le_bytes())?;
        Ok(())
    }

    pub fn read_from<R: Read>(r: &mut R) -> Result<Self, FormatError> {
        let mut magic = [0u8; 4];
        r.read_exact(&mut magic)?;
        if magic != MAGIC {
            return Err(FormatError::InvalidMagic);
        }

        let mut ver = [0u8; 1];
        r.read_exact(&mut ver)?;
        if ver[0] != FORMAT_VERSION {
            return Err(FormatError::UnsupportedVersion(ver[0]));
        }

        let mut b2 = [0u8; 2];
        let mut b4 = [0u8; 4];
        let mut b8 = [0u8; 8];

        r.read_exact(&mut b2)?;
        let flags = u16::from_le_bytes(b2);

        r.read_exact(&mut b8)?;
        let original_size = u64::from_le_bytes(b8);

        r.read_exact(&mut b4)?;
        let chunk_count = u32::from_le_bytes(b4);

        r.read_exact(&mut b8)?;
        let segment_map_offset = u64::from_le_bytes(b8);

        Ok(FileHeader { version: ver[0], flags, original_size, chunk_count, segment_map_offset })
    }
}

#[derive(Debug, Clone)]
pub struct ChunkMeta {
    pub offset_in_file: u64,
    pub original_offset: u64,
    pub original_size: u32,
    pub compressed_size: u32,
    pub data_type: DataType,
    pub transform: TransformType,
    pub codec: CodecType,
    pub checksum: u32,
}

impl ChunkMeta {
    pub const SIZE: usize = 8 + 8 + 4 + 4 + 1 + 1 + 1 + 4; // 31 bytes

    pub fn write_to<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(&self.offset_in_file.to_le_bytes())?;
        w.write_all(&self.original_offset.to_le_bytes())?;
        w.write_all(&self.original_size.to_le_bytes())?;
        w.write_all(&self.compressed_size.to_le_bytes())?;
        w.write_all(&[self.data_type as u8])?;
        w.write_all(&[self.transform as u8])?;
        w.write_all(&[self.codec as u8])?;
        w.write_all(&self.checksum.to_le_bytes())?;
        Ok(())
    }

    pub fn read_from<R: Read>(r: &mut R) -> io::Result<Self> {
        let mut b8 = [0u8; 8];
        let mut b4 = [0u8; 4];
        let mut b1 = [0u8; 1];

        r.read_exact(&mut b8)?;
        let offset_in_file = u64::from_le_bytes(b8);

        r.read_exact(&mut b8)?;
        let original_offset = u64::from_le_bytes(b8);

        r.read_exact(&mut b4)?;
        let original_size = u32::from_le_bytes(b4);

        r.read_exact(&mut b4)?;
        let compressed_size = u32::from_le_bytes(b4);

        r.read_exact(&mut b1)?;
        let data_type = DataType::from_u8(b1[0]).unwrap_or(DataType::Binary);

        r.read_exact(&mut b1)?;
        let transform = TransformType::from_u8(b1[0]).unwrap_or(TransformType::None);

        r.read_exact(&mut b1)?;
        let codec = CodecType::from_u8(b1[0]).unwrap_or(CodecType::Raw);

        r.read_exact(&mut b4)?;
        let checksum = u32::from_le_bytes(b4);

        Ok(ChunkMeta {
            offset_in_file, original_offset, original_size, compressed_size,
            data_type, transform, codec, checksum,
        })
    }
}
