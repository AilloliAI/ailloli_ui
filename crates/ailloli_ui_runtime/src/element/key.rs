#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    Static(&'static str),
    U64(u64),
    String(String),
}
