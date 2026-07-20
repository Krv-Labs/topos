//! A small, low-complexity module.
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn greet(name: &str) -> String {
    format!("hello, {name}")
}
