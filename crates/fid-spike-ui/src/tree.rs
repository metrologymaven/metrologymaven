//! The synthetic element tree: items `{ id, parent, name, kind, status }` (Design, "Data
//! structures") loaded from the fixture's TSV form, with rename and sibling reorder.

use fid_hal_idl::UntrustedText;
use std::fmt;

/// Largest accepted tree file in bytes.
pub const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
/// Largest accepted line in bytes (without the line feed).
pub const MAX_LINE_BYTES: usize = 1024;
/// Largest accepted number of items.
pub const MAX_ITEMS: usize = 200_000;
/// Largest accepted depth (roots are level 1).
pub const MAX_DEPTH: usize = 32;
/// Largest accepted item name in UTF-8 bytes.
pub const MAX_NAME_BYTES: usize = 256;
/// Largest accepted kind or status token in bytes.
pub const MAX_TOKEN_BYTES: usize = 32;

/// Why a tree file or an edit was rejected. Line numbers are 1-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeError {
    /// The file is larger than [`MAX_FILE_BYTES`].
    FileTooLarge,
    /// The file holds more than [`MAX_ITEMS`] items.
    TooManyItems,
    /// A line is longer than [`MAX_LINE_BYTES`].
    LineTooLong(usize),
    /// A line does not have exactly five tab-separated fields.
    FieldCount(usize),
    /// The id is not the next integer (ids run 1, 2, 3, ... in file order).
    BadId(usize),
    /// The parent is not 0 (root) or an earlier id.
    BadParent(usize),
    /// The item would be deeper than [`MAX_DEPTH`].
    TooDeep(usize),
    /// The kind or status token is empty, too long or not lowercase ASCII letters.
    BadToken(usize),
    /// The name is empty, longer than [`MAX_NAME_BYTES`] or holds a tab, CR or LF.
    BadName(usize),
    /// The file holds no items.
    Empty,
    /// No item has this id.
    UnknownId(u32),
    /// A reorder moved an item onto itself or next to an item with another parent.
    NotSiblings,
}

impl fmt::Display for TreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TreeError::FileTooLarge => write!(f, "tree file larger than {MAX_FILE_BYTES} bytes"),
            TreeError::TooManyItems => write!(f, "more than {MAX_ITEMS} items"),
            TreeError::LineTooLong(l) => write!(f, "line {l}: longer than {MAX_LINE_BYTES} bytes"),
            TreeError::FieldCount(l) => write!(f, "line {l}: expected 5 tab-separated fields"),
            TreeError::BadId(l) => write!(f, "line {l}: id is not the next integer"),
            TreeError::BadParent(l) => write!(f, "line {l}: parent is not 0 or an earlier id"),
            TreeError::TooDeep(l) => write!(f, "line {l}: deeper than {MAX_DEPTH} levels"),
            TreeError::BadToken(l) => write!(f, "line {l}: bad kind or status"),
            TreeError::BadName(l) => write!(f, "line {l}: bad name"),
            TreeError::Empty => write!(f, "no items"),
            TreeError::UnknownId(id) => write!(f, "unknown item id {id}"),
            TreeError::NotSiblings => write!(f, "items are not distinct siblings"),
        }
    }
}

impl std::error::Error for TreeError {}

/// One tree item. The name is untrusted file data: it is shown only through
/// [`Item::display`], the escaped form of [`UntrustedText::as_display`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// 1-based id.
    pub id: u32,
    /// Parent id; `None` for a root.
    pub parent: Option<u32>,
    /// Item name as read (or as renamed).
    pub name: UntrustedText,
    /// Kind token, for example `plane`.
    pub kind: String,
    /// Status token, for example `pass`.
    pub status: String,
    /// Level, roots are 1.
    pub level: usize,
    raw: String,
    display: String,
    search_key: String,
}

impl Item {
    /// The escaped name, safe to show.
    pub fn display(&self) -> &str {
        &self.display
    }

    /// The name with ASCII letters lowercased, for the filter box.
    pub(crate) fn search_key(&self) -> &str {
        &self.search_key
    }

    /// The text a rename starts from: the name itself when showing it needs no escaping,
    /// otherwise empty, so untrusted control or bidirectional characters never reach an
    /// edit box unescaped.
    pub fn rename_seed(&self) -> &str {
        if self.raw == self.display {
            &self.raw
        } else {
            ""
        }
    }

    fn set_name(&mut self, raw: &str) {
        self.raw = raw.to_string();
        self.name = UntrustedText::new(raw);
        self.display = self.name.as_display();
        self.search_key = raw.to_ascii_lowercase();
    }
}

/// Whether `s` is an acceptable item name: 1 to [`MAX_NAME_BYTES`] bytes, no tab, CR or LF.
pub fn valid_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_NAME_BYTES && !s.contains(['\t', '\r', '\n'])
}

fn valid_token(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_TOKEN_BYTES && s.bytes().all(|b| b.is_ascii_lowercase())
}

/// The tree: items indexed by `id - 1`, sibling order kept in `children` and `roots`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tree {
    items: Vec<Item>,
    children: Vec<Vec<u32>>,
    roots: Vec<u32>,
}

impl Tree {
    /// Parses the TSV form: `#` comment lines, then one item per line,
    /// `id<TAB>parent<TAB>kind<TAB>status<TAB>name`, ids 1, 2, 3, ... in order and every
    /// parent (0 for a root) an earlier id. A final line feed is optional.
    pub fn parse(text: &str) -> Result<Tree, TreeError> {
        if text.len() > MAX_FILE_BYTES {
            return Err(TreeError::FileTooLarge);
        }
        let mut items: Vec<Item> = Vec::new();
        let mut children: Vec<Vec<u32>> = Vec::new();
        let mut roots = Vec::new();
        let body = text.strip_suffix('\n').unwrap_or(text);
        for (n, line) in body.split('\n').enumerate() {
            let ln = n + 1;
            if line.len() > MAX_LINE_BYTES {
                return Err(TreeError::LineTooLong(ln));
            }
            if line.starts_with('#') {
                continue;
            }
            if items.len() >= MAX_ITEMS {
                return Err(TreeError::TooManyItems);
            }
            let f: Vec<&str> = line.splitn(5, '\t').collect();
            let [id, parent, kind, status, name] = f.as_slice() else {
                return Err(TreeError::FieldCount(ln));
            };
            let next = items.len() + 1;
            if *id != next.to_string() {
                return Err(TreeError::BadId(ln));
            }
            let parent: usize = match parent.parse::<usize>() {
                Ok(p) if p < next && (p == 0 || *parent == p.to_string()) => p,
                _ => return Err(TreeError::BadParent(ln)),
            };
            let level = match parent {
                0 => 1,
                p => items.get(p - 1).map_or(usize::MAX, |i| i.level + 1),
            };
            if level > MAX_DEPTH {
                return Err(TreeError::TooDeep(ln));
            }
            if !valid_token(kind) || !valid_token(status) {
                return Err(TreeError::BadToken(ln));
            }
            if !valid_name(name) {
                return Err(TreeError::BadName(ln));
            }
            let id = u32::try_from(next).map_err(|_| TreeError::TooManyItems)?;
            let parent_id = if parent == 0 { None } else { u32::try_from(parent).ok() };
            match parent_id {
                None => roots.push(id),
                Some(p) => {
                    if let Some(c) = children.get_mut(p as usize - 1) {
                        c.push(id);
                    }
                }
            }
            let mut item = Item {
                id,
                parent: parent_id,
                name: UntrustedText::default(),
                raw: String::new(),
                kind: (*kind).to_string(),
                status: (*status).to_string(),
                level,
                display: String::new(),
                search_key: String::new(),
            };
            item.set_name(name);
            items.push(item);
            children.push(Vec::new());
        }
        if items.is_empty() {
            return Err(TreeError::Empty);
        }
        Ok(Tree { items, children, roots })
    }

    /// Number of items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Always false: a parsed tree holds at least one item.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The item with this id.
    pub fn get(&self, id: u32) -> Option<&Item> {
        (id as usize).checked_sub(1).and_then(|i| self.items.get(i))
    }

    /// All items in id order.
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Children of `id` in sibling order (empty for a leaf or an unknown id).
    pub fn children(&self, id: u32) -> &[u32] {
        (id as usize).checked_sub(1).and_then(|i| self.children.get(i)).map_or(&[], |c| c.as_slice())
    }

    /// Roots in sibling order.
    pub fn roots(&self) -> &[u32] {
        &self.roots
    }

    /// Siblings of `id` (the roots for a root), including `id`.
    pub fn siblings(&self, id: u32) -> &[u32] {
        match self.get(id).and_then(|i| i.parent) {
            Some(p) => self.children(p),
            None => &self.roots,
        }
    }

    /// Largest level in the tree.
    pub fn depth(&self) -> usize {
        self.items.iter().map(|i| i.level).max().unwrap_or(0)
    }

    /// Renames `id`. The name must pass [`valid_name`].
    pub fn rename(&mut self, id: u32, name: &str) -> Result<(), TreeError> {
        if !valid_name(name) {
            return Err(TreeError::BadName(0));
        }
        let item = (id as usize).checked_sub(1).and_then(|i| self.items.get_mut(i)).ok_or(TreeError::UnknownId(id))?;
        item.set_name(name);
        Ok(())
    }

    /// Drag to reorder: moves `id` to just before its sibling `target`. Both must share a
    /// parent and differ. A parent stays before its children, so ids still grow downwards.
    pub fn move_before(&mut self, id: u32, target: u32) -> Result<(), TreeError> {
        let a = self.get(id).ok_or(TreeError::UnknownId(id))?.parent;
        let b = self.get(target).ok_or(TreeError::UnknownId(target))?.parent;
        if id == target || a != b {
            return Err(TreeError::NotSiblings);
        }
        let list = match a {
            Some(p) => self.children.get_mut(p as usize - 1).ok_or(TreeError::UnknownId(p))?,
            None => &mut self.roots,
        };
        list.retain(|&x| x != id);
        let at = list.iter().position(|&x| x == target).ok_or(TreeError::NotSiblings)?;
        list.insert(at, id);
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const SMALL: &str = "# t\n1\t0\tpart\tpass\tA\n2\t1\tplane\twarn\tB\n3\t1\tplane\tfail\tC\n4\t0\tpart\tnone\tD";

    #[test]
    fn parses_hand_written_tree() {
        let t = Tree::parse(SMALL).unwrap();
        assert_eq!(t.len(), 4);
        assert_eq!(t.roots(), &[1, 4]);
        assert_eq!(t.children(1), &[2, 3]);
        assert_eq!(t.depth(), 2);
        assert_eq!(t.get(3).unwrap().level, 2);
        assert_eq!(t.get(5), None);
        assert_eq!(t.get(0), None);
    }

    #[test]
    fn rejects_bad_input_without_panicking() {
        let cases: &[(&str, TreeError)] = &[
            ("", TreeError::FieldCount(1)),
            ("# only a comment", TreeError::Empty),
            ("2\t0\tpart\tpass\tA", TreeError::BadId(1)),
            ("01\t0\tpart\tpass\tA", TreeError::BadId(1)),
            ("1\t1\tpart\tpass\tA", TreeError::BadParent(1)),
            ("1\t0\tpart\tpass\tA\n2\t01\tpart\tpass\tB", TreeError::BadParent(2)),
            ("1\t-1\tpart\tpass\tA", TreeError::BadParent(1)),
            ("1\t0\tPart\tpass\tA", TreeError::BadToken(1)),
            ("1\t0\tpart\t\tA", TreeError::BadToken(1)),
            ("1\t0\tpart\tpass\t", TreeError::BadName(1)),
            ("1\t0\tpart\tpass\tA\tB", TreeError::BadName(1)),
            ("1\t0\tpart\tpass\tA\r", TreeError::BadName(1)),
            ("1\t0\tpart\tpass", TreeError::FieldCount(1)),
            ("1\t0\tpart\tpass\tA\n\n", TreeError::FieldCount(2)),
        ];
        for (text, want) in cases {
            assert_eq!(Tree::parse(text).as_ref().err(), Some(want), "{text:?}");
        }
        let long = format!("1\t0\tpart\tpass\t{}", "x".repeat(MAX_LINE_BYTES));
        assert_eq!(Tree::parse(&long).err(), Some(TreeError::LineTooLong(1)));
        let name = format!("1\t0\tpart\tpass\t{}", "x".repeat(MAX_NAME_BYTES + 1));
        assert_eq!(Tree::parse(&name).err(), Some(TreeError::BadName(1)));
        let mut deep = String::from("1\t0\tpart\tpass\tA\n");
        for i in 2..=MAX_DEPTH + 1 {
            deep.push_str(&format!("{i}\t{}\tpart\tpass\tA\n", i - 1));
        }
        assert_eq!(Tree::parse(&deep).err(), Some(TreeError::TooDeep(MAX_DEPTH + 1)));
    }

    #[test]
    fn names_are_shown_escaped() {
        let t = Tree::parse("1\t0\tpart\tpass\tp \u{202e}exe\\x").unwrap();
        assert_eq!(t.get(1).unwrap().display(), "p \\u{202e}exe\\\\x");
        assert_eq!(t.get(1).unwrap().name, UntrustedText::new("p \u{202e}exe\\x"));
        assert_eq!(t.get(1).unwrap().rename_seed(), "");
        let t = Tree::parse("1\t0\tpart\tpass\tplain é").unwrap();
        assert_eq!(t.get(1).unwrap().rename_seed(), "plain é");
    }

    #[test]
    fn rename_and_reorder() {
        let mut t = Tree::parse(SMALL).unwrap();
        t.rename(2, "Bolt").unwrap();
        assert_eq!(t.get(2).unwrap().display(), "Bolt");
        assert_eq!(t.rename(2, ""), Err(TreeError::BadName(0)));
        assert_eq!(t.rename(2, "a\nb"), Err(TreeError::BadName(0)));
        assert_eq!(t.rename(9, "x"), Err(TreeError::UnknownId(9)));
        t.move_before(3, 2).unwrap();
        assert_eq!(t.children(1), &[3, 2]);
        t.move_before(4, 1).unwrap();
        assert_eq!(t.roots(), &[4, 1]);
        assert_eq!(t.move_before(2, 2), Err(TreeError::NotSiblings));
        assert_eq!(t.move_before(2, 4), Err(TreeError::NotSiblings));
        assert_eq!(t.move_before(2, 7), Err(TreeError::UnknownId(7)));
        assert_eq!(t.siblings(2), &[3, 2]);
    }
}
