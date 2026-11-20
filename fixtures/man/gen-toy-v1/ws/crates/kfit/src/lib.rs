mod a;

// include_str!("../../outside.txt") in a comment is not code.
/* nor in a /* nested */ block: include!("../x.rs") */
pub const NOTE: &str = "include!(\"../x.rs\") in a string is not code";
pub const RAW: &str = r#"#[path = "../x.rs"]"#;

pub fn first<'a>(s: &'a str) -> char {
    s.chars().next().unwrap_or('"')
}

pub fn fit() -> u32 {
    a::b::two()
}
