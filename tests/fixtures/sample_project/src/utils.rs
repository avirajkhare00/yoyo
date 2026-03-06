use crate::math::add;

pub fn sum_three(a: i64, b: i64, c: i64) -> i64 {
    add(add(a, b), c)
}

pub fn clamp(value: i64, min: i64, max: i64) -> i64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

pub fn format_result(label: &str, value: i64) -> String {
    format!("{}: {}", label, value)
}
