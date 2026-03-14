pub fn greet() -> &'static str {
    "hi"
}

pub fn banner() -> String {
    format!("banner: {}", greet())
}
