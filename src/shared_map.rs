// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Copy-on-write symbol tables for lowering scopes.
use std::{collections::HashMap, ops::{Deref, DerefMut}, sync::Arc};
#[derive(Clone)]
pub(crate) struct SharedMap<V: Clone>(Arc<HashMap<String, V>>);
impl<V: Clone> Default for SharedMap<V> {
    fn default() -> Self { Self(Arc::new(HashMap::new())) }
}
impl<V: Clone> From<HashMap<String, V>> for SharedMap<V> {
    fn from(map: HashMap<String, V>) -> Self { Self(Arc::new(map)) }
}
impl<V: Clone> Deref for SharedMap<V> {
    type Target = HashMap<String, V>;
    fn deref(&self) -> &Self::Target { &self.0 }
}
impl<V: Clone> DerefMut for SharedMap<V> {
    fn deref_mut(&mut self) -> &mut Self::Target { Arc::make_mut(&mut self.0) }
}
impl<V: Clone> FromIterator<(String, V)> for SharedMap<V> {
    fn from_iter<I: IntoIterator<Item = (String, V)>>(iter: I) -> Self {
        HashMap::from_iter(iter).into()
    }
}
