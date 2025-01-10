#[cfg(feature = "reflect")]
use bevy_ecs::reflect::{
    ReflectComponent, ReflectFromWorld, ReflectMapEntities, ReflectVisitEntities,
    ReflectVisitEntitiesMut,
};
use bevy_ecs::{
    component::Component,
    entity::{Entity, VisitEntitiesMut},
    prelude::FromWorld,
    world::World,
};
use core::{ops::Deref, slice};
use smallvec::SmallVec;

/// Contains references to the child entities of this entity.
///
/// Each child must contain a [`Parent`] component that points back to this entity.
/// This component rarely needs to be created manually,
/// consider using higher level utilities like [`BuildChildren::with_children`]
/// which are safer and easier to use.
///
/// See [`HierarchyQueryExt`] for hierarchy related methods on [`Query`].
///
/// [`HierarchyQueryExt`]: crate::query_extension::HierarchyQueryExt
/// [`Query`]: bevy_ecs::system::Query
/// [`Parent`]: crate::components::parent::Parent
/// [`BuildChildren::with_children`]: crate::child_builder::BuildChildren::with_children
#[derive(Component, Debug)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[cfg_attr(
    feature = "reflect",
    reflect(
        Component,
        MapEntities,
        VisitEntities,
        VisitEntitiesMut,
        Debug,
        FromWorld
    )
)]
pub struct Children {
    pub(crate) all: SmallVec<[Entity; 8]>,
    pub(crate) active: Option<SmallVec<[Entity; 8]>>,
}

impl VisitEntitiesMut for Children {
    fn visit_entities_mut<F: FnMut(&mut Entity)>(&mut self, f: F) {
        if let Some(active) = self.active.as_mut() {
            active.as_mut().iter_mut().for_each(f);
        } else {
            self.all.iter_mut().for_each(f);
        }
    }
}

// TODO: We need to impl either FromWorld or Default so Children can be registered as Reflect.
// This is because Reflect deserialize by creating an instance and apply a patch on top.
// However Children should only ever be set with a real user-defined entities. Its worth looking
// into better ways to handle cases like this.
impl FromWorld for Children {
    #[inline]
    fn from_world(_world: &mut World) -> Self {
        Children {
            all: SmallVec::new(),
            active: None,
        }
    }
}

impl Children {
    /// Constructs a [`Children`] component with the given entities.
    #[inline]
    pub(crate) fn from_entities(entities: &[Entity]) -> Self {
        Self {
            all: SmallVec::from_slice(entities),
            active: None,
        }
    }

    /// Swaps the child at `a_index` with the child at `b_index`.
    #[inline]
    pub fn swap(&mut self, a_index: usize, b_index: usize) {
        self.all.swap(a_index, b_index);
        self.active = None;
    }

    /// Clears all the active children
    pub fn set_zero_active(&mut self) {
        if self.active.is_none() {
            self.active = Some(SmallVec::new());
        }
        let active = self.active.as_mut().unwrap();
        active.clear();
    }

    /// Returns true if there is nothing active
    pub fn is_zero_active(&self) -> bool {
        self.active.as_ref().map(|a| a.is_empty()).unwrap_or_default()
    }

    /// Adds a child entity to the active children list.
    #[inline(always)]
    pub fn add_active(&mut self, entity: Entity) {
        match &mut self.active {
            Some(active) => active.push(entity),
            None => {
                self.active = Some(SmallVec::from_slice(&[entity]));
            }
        }
    }

    /// Swaps the active entities with the provided list of entities.
    pub fn swap_active(&mut self, other: &mut SmallVec<[Entity; 8]>) {
        if self.active.is_none() {
            self.active = Some(SmallVec::new());
        }
        let active = self.active.as_mut().unwrap();
        std::mem::swap(active, other);
    }

    /// Rebuilds the children list using the provided iterator of entities.
    #[inline]
    pub fn set_active<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = Entity>,
    {
        if self.active.is_none() {
            self.active = Some(SmallVec::new());
        }
        let active = self.active.as_mut().unwrap();
        active.clear();
        active.extend(iter);
    }

    /// Sorts children [stably](https://en.wikipedia.org/wiki/Sorting_algorithm#Stability)
    /// in place using the provided comparator function.
    ///
    /// For the underlying implementation, see [`slice::sort_by`].
    ///
    /// For the unstable version, see [`sort_unstable_by`](Children::sort_unstable_by).
    ///
    /// See also [`sort_by_key`](Children::sort_by_key), [`sort_by_cached_key`](Children::sort_by_cached_key).
    #[inline]
    pub fn sort_by<F>(&mut self, mut compare: F)
    where
        F: FnMut(&Entity, &Entity) -> core::cmp::Ordering,
    {
        self.all.sort_by(&mut compare);
        if let Some(active) = self.active.as_mut() {
            active.sort_by(&mut compare);
        }
    }

    /// Sorts children [stably](https://en.wikipedia.org/wiki/Sorting_algorithm#Stability)
    /// in place using the provided key extraction function.
    ///
    /// For the underlying implementation, see [`slice::sort_by_key`].
    ///
    /// For the unstable version, see [`sort_unstable_by_key`](Children::sort_unstable_by_key).
    ///
    /// See also [`sort_by`](Children::sort_by), [`sort_by_cached_key`](Children::sort_by_cached_key).
    #[inline]
    pub fn sort_by_key<K, F>(&mut self, mut compare: F)
    where
        F: FnMut(&Entity) -> K,
        K: Ord,
    {
        self.all.sort_by_key(&mut compare);
        if let Some(active) = self.active.as_mut() {
            active.sort_by_key(&mut compare);
        }
    }

    /// Sorts children [stably](https://en.wikipedia.org/wiki/Sorting_algorithm#Stability)
    /// in place using the provided key extraction function. Only evaluates each key at most
    /// once per sort, caching the intermediate results in memory.
    ///
    /// For the underlying implementation, see [`slice::sort_by_cached_key`].
    ///
    /// See also [`sort_by`](Children::sort_by), [`sort_by_key`](Children::sort_by_key).
    #[inline]
    pub fn sort_by_cached_key<K, F>(&mut self, mut compare: F)
    where
        F: FnMut(&Entity) -> K,
        K: Ord,
    {
        self.all.sort_by_cached_key(&mut compare);
        if let Some(active) = self.active.as_mut() {
            active.sort_by_cached_key(&mut compare);
        }
    }

    /// Sorts children [unstably](https://en.wikipedia.org/wiki/Sorting_algorithm#Stability)
    /// in place using the provided comparator function.
    ///
    /// For the underlying implementation, see [`slice::sort_unstable_by`].
    ///
    /// For the stable version, see [`sort_by`](Children::sort_by).
    ///
    /// See also [`sort_unstable_by_key`](Children::sort_unstable_by_key).
    #[inline]
    pub fn sort_unstable_by<F>(&mut self, mut compare: F)
    where
        F: FnMut(&Entity, &Entity) -> core::cmp::Ordering,
    {
        self.all.sort_unstable_by(&mut compare);
        if let Some(active) = self.active.as_mut() {
            active.sort_unstable_by(&mut compare);
        }
    }

    /// Sorts children [unstably](https://en.wikipedia.org/wiki/Sorting_algorithm#Stability)
    /// in place using the provided key extraction function.
    ///
    /// For the underlying implementation, see [`slice::sort_unstable_by_key`].
    ///
    /// For the stable version, see [`sort_by_key`](Children::sort_by_key).
    ///
    /// See also [`sort_unstable_by`](Children::sort_unstable_by).
    #[inline]
    pub fn sort_unstable_by_key<K, F>(&mut self, mut compare: F)
    where
        F: FnMut(&Entity) -> K,
        K: Ord,
    {
        self.all.sort_unstable_by_key(&mut compare);
        if let Some(active) = self.active.as_mut() {
            active.sort_unstable_by_key(&mut compare);
        }
    }
}

impl Deref for Children {
    type Target = [Entity];

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        if let Some(active) = self.active.as_ref() {
            &active[..]
        } else {
            &self.all[..]
        }
    }
}

impl<'a> IntoIterator for &'a Children {
    type Item = <Self::IntoIter as Iterator>::Item;

    type IntoIter = slice::Iter<'a, Entity>;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        if let Some(active) = self.active.as_ref() {
            active.iter()
        } else {
            self.all.iter()
        }
    }
}
