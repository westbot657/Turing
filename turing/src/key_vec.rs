use std::{ops::{Deref, Index, IndexMut}, slice::SliceIndex};

/// A grow-only vector structure indexed by key types.
pub struct KeyVec<K, V>
where
    K: From<u32> + Into<u32> + Clone,
{
    values: Vec<V>,
    marker: std::marker::PhantomData<K>,
}

impl<K, V> KeyVec<K, V>
where
    K: From<u32> + Into<u32> + Clone,
{
    /// Creates a new empty KeyVec.
    pub fn new() -> Self {
        Self {
            values: Vec::new(),
            marker: std::marker::PhantomData,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            values: Vec::with_capacity(capacity),
            marker: std::marker::PhantomData,
        }
    }

    /// Pushes a new value to the KeyVec and returns its key.
    pub fn push(&mut self, value: V) -> K {
        let key = K::from(self.values.len() as u32);
        self.values.push(value);
        key
    }

    pub fn key_of<F>(&self, f: F) -> Option<K>
    where
        F: Fn(&V) -> bool,
    {
        for (i, v) in self.values.iter().enumerate() {
            if f(v) {
                return Some(K::from(i as u32));
            }
        }
        None
    }

    #[inline]
    pub fn get(&self, key: &K) -> Option<&V> {
        self.values.get(key.clone().into() as usize)
    }

    #[inline]
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.values.get_mut(key.clone().into() as usize)
    }

    /// Clears all values from the KeyVec.
    /// Invalidates all existing keys.
    pub fn clear(&mut self) {
        self.values.clear();
    }
}

impl<K, V> Index<K> for KeyVec<K, V>
where
    K: From<u32> + Into<u32> + Clone,
{
    type Output = V;

    fn index(&self, index: K) -> &Self::Output {
        &self.values[index.into() as usize]
    }
}

impl<K, V> IndexMut<K> for KeyVec<K, V>
where
    K: From<u32> + Into<u32> + Clone,
{
    fn index_mut(&mut self, index: K) -> &mut Self::Output {
        &mut self.values[index.into() as usize]
    }
}

impl<K, V> Deref for KeyVec<K, V>
where
    K: From<u32> + Into<u32> + Clone,
{
    type Target = Vec<V>;

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl<K, V> Default for KeyVec<K, V>
where
    K: From<u32> + Into<u32> + Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> FromIterator<V> for KeyVec<K, V>
where
    u32: From<K>,
    K: From<u32> + Clone,
{
    fn from_iter<T: IntoIterator<Item = V>>(iter: T) -> Self {
        let mut kv = KeyVec::new();
        for v in iter {
            kv.push(v);
        }
        kv
    }
}

impl<K, V> IntoIterator for KeyVec<K, V>
where
    K: From<u32> + Into<u32> + Clone,
{
    type Item = V;
    type IntoIter = std::vec::IntoIter<V>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.into_iter()
    }
}

impl<K, V> Clone for KeyVec<K, V>
where
    K: From<u32> + Into<u32> + Clone,
    V: Clone,
{
    fn clone(&self) -> Self {
        Self {
            values: self.values.clone(),
            marker: std::marker::PhantomData,
        }
    }
}