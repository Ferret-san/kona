//! Macros used across test utilities.

/// A shorthand syntax for constructing [op_alloy_protocol::Frame]s.
#[macro_export]
macro_rules! frame {
    ($id:expr, $number:expr, $data:expr, $is_last:expr) => {
        op_alloy_protocol::Frame { id: [$id; 16], number: $number, data: $data, is_last: $is_last }
    };
}
