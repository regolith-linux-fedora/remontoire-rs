//! The expanded categories, saved in `expanded-category-path-ids` as one
//! comma-separated string. Category names can contain commas, so `%` and
//! `,` in names are saved as `%25` and `%2C`; every name is followed by a
//! comma.
//!
//! The saved order is the iteration order of the hash set the original
//! implementation kept (libgee's `HashSet` with GLib's string hash), which
//! [`CategorySet`] reproduces so the stored value is byte-identical.

/// libgee `HashSet` bucket counts: `SpacedPrimes`.
const SPACED_PRIMES: [usize; 34] = [
    11, 19, 37, 73, 109, 163, 251, 367, 557, 823, 1237, 1861, 2777, 4177, 6247, 9371, 14057, 21089,
    31627, 47431, 71143, 106721, 160073, 240101, 360163, 540217, 810343, 1215497, 1823231, 2734867,
    4102283, 6153409, 9230113, 13845163,
];
const MIN_SIZE: usize = 11;
const MAX_SIZE: usize = 13845163;

/// `g_str_hash` (djb2 over signed chars).
fn str_hash(text: &str) -> u32 {
    text.bytes().fold(5381u32, |hash, byte| {
        // GLib adds each byte as a signed char.
        hash.wrapping_mul(33)
            .wrapping_add_signed(i32::from(byte as i8))
    })
}

fn closest_prime(count: usize) -> usize {
    SPACED_PRIMES
        .iter()
        .copied()
        .find(|&prime| prime > count)
        .unwrap_or(MAX_SIZE)
}

/// A set of category names that iterates like libgee's `HashSet<string>`:
/// chained buckets, appended on insert, rehashed (reversing chains) when the
/// load leaves the 1/3..3 band.
#[derive(Debug)]
pub struct CategorySet {
    buckets: Vec<Vec<(u32, String)>>,
    len: usize,
}

impl Default for CategorySet {
    fn default() -> Self {
        Self {
            buckets: vec![Vec::new(); MIN_SIZE],
            len: 0,
        }
    }
}

impl CategorySet {
    fn bucket(&self, hash: u32) -> usize {
        hash as usize % self.buckets.len()
    }

    pub fn contains(&self, name: &str) -> bool {
        let hash = str_hash(name);
        self.buckets[self.bucket(hash)]
            .iter()
            .any(|(h, n)| *h == hash && n == name)
    }

    /// Add `name`; false if it was already present.
    pub fn insert(&mut self, name: &str) -> bool {
        if self.contains(name) {
            return false;
        }
        let hash = str_hash(name);
        let index = self.bucket(hash);
        self.buckets[index].push((hash, name.to_owned()));
        self.len += 1;
        self.resize();
        true
    }

    /// Remove `name`; false if it was absent.
    pub fn remove(&mut self, name: &str) -> bool {
        let hash = str_hash(name);
        let index = self.bucket(hash);
        let Some(position) = self.buckets[index]
            .iter()
            .position(|(h, n)| *h == hash && n == name)
        else {
            return false;
        };
        self.buckets[index].remove(position);
        self.len -= 1;
        self.resize();
        true
    }

    fn resize(&mut self) {
        let size = self.buckets.len();
        if !((size >= 3 * self.len && size >= MIN_SIZE)
            || (3 * size <= self.len && size < MAX_SIZE))
        {
            return;
        }
        let new_size = closest_prime(self.len).clamp(MIN_SIZE, MAX_SIZE);
        let mut buckets: Vec<Vec<(u32, String)>> = vec![Vec::new(); new_size];
        for chain in std::mem::take(&mut self.buckets) {
            for node in chain {
                // Rehashing pushes each node onto the front of its chain.
                buckets[node.0 as usize % new_size].insert(0, node);
            }
        }
        self.buckets = buckets;
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.buckets
            .iter()
            .flat_map(|chain| chain.iter().map(|(_, name)| name.as_str()))
    }
}

/// The saved form of `categories`.
pub fn join(categories: &CategorySet) -> String {
    let mut saved = String::new();
    for name in categories.iter() {
        saved.push_str(&name.replace('%', "%25").replace(',', "%2C"));
        saved.push(',');
    }
    saved
}

/// The categories in a saved string; empty entries are skipped.
pub fn split(saved: &str) -> CategorySet {
    let mut categories = CategorySet::default();
    for entry in saved.split(',').filter(|entry| !entry.is_empty()) {
        categories.insert(&entry.replace("%2C", ",").replace("%25", "%"));
    }
    categories
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_g_str_hash() {
        assert_eq!(str_hash(""), 5381);
        assert_eq!(str_hash("a"), 5381 * 33 + 97);
        // Bytes above 0x7f are added as negative signed chars.
        assert_eq!(str_hash("\u{e9}"), (5381u32 * 33 - 61) * 33 - 87);
    }

    #[test]
    fn names_round_trip() {
        let mut set = CategorySet::default();
        for name in ["Alpha,Beta", "100% Sure", "Tricky%2CLiteral", "Plain"] {
            set.insert(name);
        }
        let saved = join(&set);
        assert!(saved.contains("Alpha%2CBeta,"));
        assert!(saved.contains("100%25 Sure,"));
        assert!(saved.contains("Tricky%252CLiteral,"));
        assert!(saved.ends_with(','));
        let restored = split(&saved);
        assert_eq!(
            restored.iter().collect::<Vec<_>>(),
            set.iter().collect::<Vec<_>>()
        );
    }

    #[test]
    fn split_skips_empty_entries_and_decodes_once() {
        let set = split(",Alpha,,Tricky%252CLiteral,100%25 Sure,,");
        let mut names: Vec<&str> = set.iter().collect();
        names.sort_unstable();
        assert_eq!(names, ["100% Sure", "Alpha", "Tricky%2CLiteral"]);
    }

    #[test]
    fn insert_and_remove_track_membership() {
        let mut set = CategorySet::default();
        assert!(set.insert("a"));
        assert!(!set.insert("a"));
        assert!(set.contains("a"));
        assert!(set.remove("a"));
        assert!(!set.remove("a"));
        assert_eq!(join(&set), "");
    }

    #[test]
    fn grows_and_shrinks_keeping_members() {
        let mut set = CategorySet::default();
        let names: Vec<String> = (0..200).map(|i| format!("category {i}")).collect();
        for name in &names {
            set.insert(name);
        }
        assert!(set.buckets.len() > MIN_SIZE);
        assert_eq!(set.iter().count(), 200);
        for name in &names[..195] {
            set.remove(name);
        }
        assert_eq!(set.buckets.len(), MIN_SIZE);
        let mut left: Vec<&str> = set.iter().collect();
        left.sort_unstable();
        let mut expected: Vec<&str> = names[195..].iter().map(String::as_str).collect();
        expected.sort_unstable();
        assert_eq!(left, expected);
    }
}
