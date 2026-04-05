pub mod bitplane;
pub mod bwt;
pub mod delta;
pub mod float_split;
pub mod rle;
pub mod transpose;

use crate::format::{DataType, TransformType};

/// Select the best transform for a given data type.
pub fn select_transform(data_type: DataType) -> TransformType {
    match data_type {
        DataType::Text => TransformType::Bwt,
        DataType::Structured => TransformType::Transpose,
        DataType::Binary => TransformType::None, // LZ handles this in entropy layer
        DataType::NumericInt => TransformType::Delta,
        DataType::NumericFloat => TransformType::FloatSplit,
        DataType::CompressedOrRandom => TransformType::None,
        DataType::Sparse => TransformType::Rle,
    }
}

/// Apply the selected transform to data. Returns transformed data.
pub fn apply_transform(data: &[u8], transform: TransformType) -> Vec<u8> {
    match transform {
        TransformType::None => data.to_vec(),
        TransformType::Bwt => bwt::forward(data),
        TransformType::Delta => delta::encode(data),
        TransformType::FloatSplit => float_split::encode(data),
        TransformType::Transpose => transpose::encode(data),
        TransformType::Rle => rle::encode(data),
        TransformType::BitPlane => bitplane::encode(data),
    }
}

/// Reverse the transform. Returns original data.
pub fn reverse_transform(data: &[u8], transform: TransformType) -> Vec<u8> {
    match transform {
        TransformType::None => data.to_vec(),
        TransformType::Bwt => bwt::inverse(data),
        TransformType::Delta => delta::decode(data),
        TransformType::FloatSplit => float_split::decode(data),
        TransformType::Transpose => transpose::decode(data),
        TransformType::Rle => rle::decode(data),
        TransformType::BitPlane => bitplane::decode(data),
    }
}
