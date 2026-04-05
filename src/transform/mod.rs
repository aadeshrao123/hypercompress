pub mod bcj;
pub mod bitplane;
pub mod bwt;
pub mod delta;
pub mod float_split;
pub mod mtf;
pub mod precomp;
pub mod prediction;
pub mod rle;
pub mod struct_split;
pub mod transpose;
pub mod zero_rle;

use crate::format::{DataType, TransformType};

pub fn select_transform(data_type: DataType) -> TransformType {
    match data_type {
        DataType::Text => TransformType::BwtMtf,
        DataType::Structured => TransformType::BwtMtf,
        DataType::Binary => TransformType::Prediction,
        DataType::NumericInt => TransformType::StructSplit,
        DataType::NumericFloat => TransformType::FloatSplit,
        DataType::CompressedOrRandom => TransformType::None,
        DataType::Sparse => TransformType::Rle,
    }
}

pub fn apply_transform(data: &[u8], transform: TransformType) -> Vec<u8> {
    match transform {
        TransformType::None => data.to_vec(),
        TransformType::Bwt => bwt::forward(data),
        TransformType::Delta => delta::encode(data),
        TransformType::FloatSplit => float_split::encode(data),
        TransformType::Transpose => transpose::encode(data),
        TransformType::Rle => rle::encode(data),
        TransformType::BitPlane => bitplane::encode(data),
        TransformType::BwtMtf => {
            let bwt_out = bwt::forward(data);
            if bwt_out.len() <= 4 {
                return bwt_out;
            }
            let mut out = Vec::with_capacity(bwt_out.len());
            out.extend_from_slice(&bwt_out[..4]);
            let mtf_out = mtf::encode(&bwt_out[4..]);
            let zrle_out = zero_rle::encode(&mtf_out);
            out.extend_from_slice(&zrle_out);
            out
        }
        TransformType::Prediction => prediction::encode(data),
        TransformType::StructSplit => struct_split::encode(data),
        TransformType::Bcj => bcj::encode(data),
        TransformType::Precomp => precomp::encode(data),
    }
}

pub fn reverse_transform(data: &[u8], transform: TransformType) -> Vec<u8> {
    match transform {
        TransformType::None => data.to_vec(),
        TransformType::Bwt => bwt::inverse(data),
        TransformType::Delta => delta::decode(data),
        TransformType::FloatSplit => float_split::decode(data),
        TransformType::Transpose => transpose::decode(data),
        TransformType::Rle => rle::decode(data),
        TransformType::BitPlane => bitplane::decode(data),
        TransformType::BwtMtf => {
            if data.len() <= 4 {
                return bwt::inverse(data);
            }
            let header = &data[..4];
            let zrle_dec = zero_rle::decode(&data[4..]);
            let mtf_dec = mtf::decode(&zrle_dec);
            let mut bwt_data = Vec::with_capacity(4 + mtf_dec.len());
            bwt_data.extend_from_slice(header);
            bwt_data.extend_from_slice(&mtf_dec);
            bwt::inverse(&bwt_data)
        }
        TransformType::Prediction => prediction::decode(data),
        TransformType::StructSplit => struct_split::decode(data),
        TransformType::Bcj => bcj::decode(data),
        TransformType::Precomp => precomp::decode(data),
    }
}
