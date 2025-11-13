use serde::{Deserialize, Serialize};

use crate::engine::meta::IndexMeta;


#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Tags(u64);

impl Tags {
    pub const WIKI: u64 = 1 << 0;
    pub const NEWS: u64 = 1 << 1;
    pub const SNS: u64 = 1 << 2;
    pub const BLOG: u64 = 1 << 3;
    pub const FORUM: u64 = 1 << 4;
    pub const SHOPPING: u64 = 1 << 5;
    pub const ACADEMIC: u64 = 1 << 6;
    pub const TOOLS: u64 = 1 << 7;

    /// すべて満たしてるか
    /// tagはselfに含まれている必要がある
    /// eg: is_filter_contains(Tags::NEWS | Tags::BLOG) -> NEWSとBLOGの両方を含む場合のみtrue
    pub fn is_filter_contains<T: Into<u64> + Copy>(&self, tag: T) -> bool {
        (self.0 & tag.into()) == tag.into()
    }

    pub fn contains<T: Into<u64>>(&self, tag: T) -> bool {
        (self.0 & tag.into()) != 0
    }

    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    pub fn tags(&self) -> Vec<Box<str>> {
        let mut result = Vec::new();
        if self.contains(Self::WIKI) { result.push("WIKI".into()); }
        if self.contains(Self::NEWS) { result.push("NEWS".into()); }
        if self.contains(Self::SNS) { result.push("SNS".into()); }
        if self.contains(Self::BLOG) { result.push("BLOG".into()); }
        if self.contains(Self::FORUM) { result.push("FORUM".into()); }
        if self.contains(Self::SHOPPING) { result.push("SHOPPING".into()); }
        if self.contains(Self::ACADEMIC) { result.push("ACADEMIC".into()); }
        if self.contains(Self::TOOLS) { result.push("TOOLS".into()); }
        result
    }

    pub fn from_strs<T>(tags: &[T]) -> Self
    where
        T: AsRef<str>,
    {
        let mut set = 0;
        for tag in tags {
            let s = tag.as_ref();
            if s.eq_ignore_ascii_case("wiki")      { set |= Self::WIKI; }
            else if s.eq_ignore_ascii_case("news") { set |= Self::NEWS; }
            else if s.eq_ignore_ascii_case("sns")  { set |= Self::SNS; }
            else if s.eq_ignore_ascii_case("blog") { set |= Self::BLOG; }
            else if s.eq_ignore_ascii_case("forum"){ set |= Self::FORUM; }
            else if s.eq_ignore_ascii_case("shopping"){ set |= Self::SHOPPING; }
            else if s.eq_ignore_ascii_case("academic"){ set |= Self::ACADEMIC; }
            else if s.eq_ignore_ascii_case("tools"){ set |= Self::TOOLS; }
        }
        Self(set)
    }
}

impl PartialEq for IndexMeta {
    fn eq(&self, other: &Self) -> bool {
        self.url == other.url
    }
}

impl Into<u64> for Tags {
    fn into(self) -> u64 {
        self.0
    }
}