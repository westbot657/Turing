use std::collections::{HashMap, VecDeque};
use std::ops::RangeInclusive;



pub struct TrackedHashMap<T> {
    map: HashMap<u32, T>,
    free_ranges: VecDeque<RangeInclusive<u32>>,
    next_index: u32,
    start: u32,
}

impl<T> TrackedHashMap<T> {
    pub fn new() -> Self {
        Self::starting_at(0)
    }

    pub fn starting_at(start: u32) -> Self {
        Self {
            map: HashMap::new(),
            free_ranges: VecDeque::new(),
            next_index: start,
            start,
        }
    }

    pub fn add(&mut self, val: T) -> u32 {
        let i = if let Some(range) = self.free_ranges.front_mut() {
            let vl = *range.start();
            if vl < self.start {
                // Skip any freed indices below start
                *range = (self.start)..=*range.end();
            }
            let vl = *range.start();
            *range = (*range.start() + 1)..=*range.end();
            if range.start() > range.end() {
                self.free_ranges.pop_front();
            }
            vl
        } else {
            let mut i = self.next_index;
            while self.map.contains_key(&i) || i < self.start {
                self.next_index += 1;
                if self.next_index == u32::MAX {
                    panic!("Out of memory");
                }
                i = self.next_index;
            }
            self.next_index += 1;
            i
        };

        self.map.insert(i, val);
        i
    }

    pub fn get(&self, i: &u32) -> Option<&T> {
        self.map.get(i)
    }

    pub fn get_mut(&mut self, i: &u32) -> Option<&mut T> {
        self.map.get_mut(i)
    }

    pub fn remove(&mut self, i: &u32) -> Option<T> {
        if *i >= self.start {
            self.insert_free_range(*i);
        }
        self.map.remove(i)
    }

    fn insert_free_range(&mut self, i: u32) {
        if i < self.start {
            return; // don't track free indices below start
        }

        let mut inserted = false;
        for idx in 0..self.free_ranges.len() {
            let range = &mut self.free_ranges[idx];
            if i + 1 == *range.start() {
                *range = i..=*range.end();
                inserted = true;
                break;
            } else if i == *range.end() + 1 {
                *range = *range.start()..=i;
                inserted = true;
                break;
            } else if i < *range.start() {
                self.free_ranges.insert(idx, i..=i);
                inserted = true;
                break;
            }
        }

        if !inserted {
            self.free_ranges.push_back(i..=i);
        }

        // Merge overlapping ranges
        let mut merged = VecDeque::new();
        while let Some(mut current) = self.free_ranges.pop_front() {
            while let Some(next) = self.free_ranges.front() {
                if *current.end() + 1 >= *next.start() {
                    let next = self.free_ranges.pop_front().unwrap();
                    current = *current.start()..=*next.end();
                } else {
                    break;
                }
            }
            merged.push_back(current);
        }
        self.free_ranges = merged;
    }
}




