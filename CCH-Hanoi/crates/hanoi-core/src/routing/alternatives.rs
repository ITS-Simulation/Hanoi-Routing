pub use rust_road_router::algo::customizable_contraction_hierarchy::query::alternative::{
    AlternativeRoute, AlternativeServer, DEFAULT_STRETCH,
};
use crate::{CoordRejection, QueryAnswer};

/// Max geographic distance ratio: reject alternatives whose geo distance exceeds shortest × this factor.
/// Loại tuyến thay thế nếu quãng đường thực tế dài hơn tuyến ngắn nhất quá số lần này.
pub const MAX_GEO_RATIO: f64 = 2.0;

/// Over-request multiplier: request this many times max_alternatives from the algorithm so that
/// geographic filtering still leaves enough candidates.
/// Hệ số yêu cầu dư: xin nhiều hơn số tuyến cần thiết để sau khi lọc địa lý vẫn còn đủ kết quả.
pub const GEO_OVER_REQUEST: usize = 3;

pub trait MultiQueryRepository {
    fn run_multi_query(
        &mut self,
        from: u32,
        to: u32,
        alternatives: usize,
        stretch: f64,
    ) -> Vec<QueryAnswer>;

    fn run_multi_query_coords(
        &mut self,
        from: (f32, f32),
        to: (f32, f32),
        alternatives: usize,
        stretch: f64,
    ) -> Result<Vec<QueryAnswer>, CoordRejection>;
}
