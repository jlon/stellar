//! 集合扩展工具模块
//!
//! 提供常用的集合处理 lambda 表达式和辅助函数

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// 将 Vec 转换为 HashMap，使用指定的 key 提取函数
///
/// # Example
/// ```ignore
/// let users: Vec<User> = ...;
/// let user_map = vec_to_map(users, |u| u.id);
/// ```
#[inline]
pub fn vec_to_map<T, K, F>(items: Vec<T>, key_fn: F) -> HashMap<K, T>
where
    K: Eq + Hash,
    F: Fn(&T) -> K,
{
    items.into_iter().map(|item| (key_fn(&item), item)).collect()
}

/// 将 Vec 转换为 HashMap，使用指定的 key 和 value 提取函数
///
/// # Example
/// ```ignore
/// let rows: Vec<Row> = ...;
/// let name_to_type = vec_to_map_with(rows, |r| r.name.clone(), |r| r.object_type);
/// ```
#[inline]
pub fn vec_to_map_with<T, K, V, KF, VF>(items: Vec<T>, key_fn: KF, value_fn: VF) -> HashMap<K, V>
where
    K: Eq + Hash,
    KF: Fn(&T) -> K,
    VF: Fn(&T) -> V,
{
    items
        .into_iter()
        .map(|item| (key_fn(&item), value_fn(&item)))
        .collect()
}

/// 将 Vec 按 key 分组
///
/// # Example
/// ```ignore
/// let items: Vec<Item> = ...;
/// let grouped = group_by(items, |i| i.category.clone());
/// ```
#[inline]
pub fn group_by<T, K, F>(items: Vec<T>, key_fn: F) -> HashMap<K, Vec<T>>
where
    K: Eq + Hash,
    F: Fn(&T) -> K,
{
    let mut map: HashMap<K, Vec<T>> = HashMap::new();
    for item in items {
        map.entry(key_fn(&item)).or_default().push(item);
    }
    map
}

/// 去重并保持顺序
///
/// # Example
/// ```ignore
/// let ids = vec![1, 2, 1, 3, 2];
/// let unique = unique_ordered(ids); // [1, 2, 3]
/// ```
#[inline]
pub fn unique_ordered<T: Eq + Hash + Clone>(items: Vec<T>) -> Vec<T> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(item.clone()))
        .collect()
}

/// 集合差集操作的便捷函数
///
/// # Example
/// ```ignore
/// let current = vec![1, 2, 3];
/// let new_set = vec![2, 3, 4];
/// let (to_add, to_remove) = diff_sets(&current, &new_set);
/// // to_add: [4], to_remove: [1]
/// ```
pub fn diff_sets<T: Eq + Hash + Clone>(current: &[T], new_items: &[T]) -> (Vec<T>, Vec<T>) {
    let current_set: HashSet<_> = current.iter().cloned().collect();
    let new_set: HashSet<_> = new_items.iter().cloned().collect();
    
    let to_add: Vec<T> = new_set.difference(&current_set).cloned().collect();
    let to_remove: Vec<T> = current_set.difference(&new_set).cloned().collect();
    
    (to_add, to_remove)
}

/// Iterator 扩展 trait
pub trait IteratorExt: Iterator {
    /// 过滤并映射，跳过 None 值
    fn filter_map_some<B, F>(self, f: F) -> impl Iterator<Item = B>
    where
        Self: Sized,
        F: FnMut(Self::Item) -> Option<B>;
}

impl<I: Iterator> IteratorExt for I {
    #[inline]
    fn filter_map_some<B, F>(self, f: F) -> impl Iterator<Item = B>
    where
        F: FnMut(Self::Item) -> Option<B>,
    {
        self.filter_map(f)
    }
}
