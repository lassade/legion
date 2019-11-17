use crate::{
    entity::{Entity, EntityAllocator},
    storage::{
        ArchetypeData, ArchetypeDescription, Chunkset, ComponentMeta, ComponentResourceSet,
        ComponentStorage, ComponentTypeId, TagMeta, TagStorage, TagTypeId, Tags,
    },
    world::World,
};
use serde::{
    self,
    de::{self, DeserializeSeed, Visitor},
    Deserialize, Deserializer,
};
use std::{cell::RefCell, collections::HashMap, ptr::NonNull};

pub fn deserialize<'dd, 'a, 'b, CS: WorldDeserializer, D: Deserializer<'dd>>(
    world: &'a mut World,
    deserialize_impl: &'b CS,
    deserializer: D,
) -> Result<(), <D as Deserializer<'dd>>::Error> {
    let world_refcell = RefCell::new(world);
    let deserializable = WorldDeserialize {
        world: &world_refcell,
        user: deserialize_impl,
    };
    <WorldDeserialize<CS> as DeserializeSeed>::deserialize(deserializable, deserializer)
}

pub trait WorldDeserializer {
    fn deserialize_archetype_description<'de, D: Deserializer<'de>>(
        &self,
        deserializer: D,
    ) -> Result<ArchetypeDescription, <D as Deserializer<'de>>::Error>;
    fn deserialize_components<'de, D: Deserializer<'de>>(
        &self,
        deserializer: D,
        component_type: &ComponentTypeId,
        component_meta: &ComponentMeta,
        write_components: &mut dyn FnMut(NonNull<u8>, usize),
    ) -> Result<(), <D as Deserializer<'de>>::Error>;
    fn deserialize_tags<'de, D: Deserializer<'de>>(
        &self,
        deserializer: D,
        tag_type: &TagTypeId,
        tag_meta: &TagMeta,
        tags: &mut TagStorage,
    ) -> Result<(), <D as Deserializer<'de>>::Error>;
    fn deserialize_entities<'de, D: Deserializer<'de>>(
        &self,
        deserializer: D,
        entity_allocator: &mut EntityAllocator,
        entities: &mut Vec<Entity>,
    ) -> Result<(), <D as Deserializer<'de>>::Error>;
}

struct WorldDeserialize<'a, 'b, WD: WorldDeserializer> {
    user: &'b WD,
    world: &'a RefCell<&'a mut World>,
}
impl<'de, 'a, 'b, WD: WorldDeserializer> DeserializeSeed<'de> for WorldDeserialize<'a, 'b, WD> {
    type Value = ();
    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(SeqDeserializer(ArchetypeDeserializer {
            user: self.user,
            world: self.world,
        }))?;
        Ok(())
    }
}
#[derive(Deserialize, Debug)]
#[serde(field_identifier, rename_all = "snake_case")]
enum ArchetypeField {
    Description,
    Tags,
    ChunkSets,
}
struct ArchetypeDeserializer<'a, 'b, WD: WorldDeserializer> {
    user: &'b WD,
    world: &'a RefCell<&'a mut World>,
}
impl<'a, 'b, WD: WorldDeserializer> Clone for ArchetypeDeserializer<'a, 'b, WD> {
    fn clone(&self) -> Self {
        Self {
            user: self.user,
            world: self.world,
        }
    }
}
impl<'de, 'a, 'b, WD: WorldDeserializer> DeserializeSeed<'de>
    for ArchetypeDeserializer<'a, 'b, WD>
{
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        impl<'a, 'b, 'de, WD: WorldDeserializer> Visitor<'de> for ArchetypeDeserializer<'a, 'b, WD> {
            type Value = ();

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("struct Archetype")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
            where
                V: de::MapAccess<'de>,
            {
                let mut archetype_idx = None;
                let mut chunkset_map = None;
                while let Some(key) = map.next_key()? {
                    match key {
                        ArchetypeField::Description => {
                            println!("desc");
                            archetype_idx =
                                Some(map.next_value_seed(ArchetypeDescriptionDeserialize {
                                    user: self.user,
                                    world: self.world,
                                })?);
                        }
                        ArchetypeField::Tags => {
                            println!("tags");
                            let archetype_idx =
                                archetype_idx.expect("expected archetype description before tags");
                            let mut world = self.world.borrow_mut();
                            let archetype_data =
                                &mut world.storage_mut().archetypes_mut()[archetype_idx];
                            chunkset_map = Some(map.next_value_seed(TagsDeserializer {
                                user: self.user,
                                archetype: archetype_data,
                            })?);
                        }
                        ArchetypeField::ChunkSets => {
                            println!("chunk_set");
                            let archetype_idx = archetype_idx
                                .expect("expected archetype description before chunksets");
                            let mut world = self.world.borrow_mut();
                            map.next_value_seed(ChunkSetDeserializer {
                                user: self.user,
                                world: &mut *world,
                                archetype_idx,
                                chunkset_map: chunkset_map
                                    .as_ref()
                                    .expect("expected tags before chunksets"),
                            })?;
                            return Ok(());
                        }
                    }
                }
                Err(de::Error::missing_field("data"))
            }
        }
        println!("deserialize struct");
        const FIELDS: &'static [&'static str] = &["description", "tags", "chunk_sets"];
        deserializer.deserialize_struct("Archetype", FIELDS, self)
    }
}

pub struct SeqDeserializer<T>(T);

impl<'de, T: DeserializeSeed<'de> + Clone> DeserializeSeed<'de> for SeqDeserializer<T> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(self)
    }
}
impl<'de, T: DeserializeSeed<'de> + Clone> Visitor<'de> for SeqDeserializer<T> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("sequence of objects")
    }
    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        while let Some(_) = seq.next_element_seed::<T>(self.0.clone())? {}
        Ok(())
    }
}
struct ArchetypeDescriptionDeserialize<'a, 'b, WD: WorldDeserializer> {
    user: &'b WD,
    world: &'a RefCell<&'a mut World>,
}
impl<'de, 'a, 'b, WD: WorldDeserializer> DeserializeSeed<'de>
    for ArchetypeDescriptionDeserialize<'a, 'b, WD>
{
    type Value = usize;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let archetype_desc = <WD as WorldDeserializer>::deserialize_archetype_description::<D>(
            self.user,
            deserializer,
        )?;
        let mut world = self.world.borrow_mut();
        let mut storage = world.storage_mut();
        Ok(storage
            .archetypes()
            .iter()
            .position(|a| a.description() == &archetype_desc)
            .unwrap_or_else(|| {
                println!(" alloc archetype");
                let (idx, _) = storage.alloc_archetype(archetype_desc);
                idx
            }))
    }
}

type ChunkSetMapping = HashMap<usize, usize>;

struct TagsDeserializer<'a, 'b, WD: WorldDeserializer> {
    user: &'b WD,
    archetype: &'a mut ArchetypeData,
}
impl<'de, 'a, 'b, WD: WorldDeserializer> DeserializeSeed<'de> for TagsDeserializer<'a, 'b, WD> {
    type Value = ChunkSetMapping;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        println!("wut");
        let (mut deserialized_tags, this) = deserializer.deserialize_seq(self)?;
        let tag_types = this
            .archetype
            .description()
            .tags()
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let mut chunkset_map = ChunkSetMapping::new();
        let mut world_tag_storages = Vec::new();
        for (tag_type, _) in tag_types.iter() {
            // TODO fix mutability issue in the storage API here?
            let tags = unsafe { &mut *(this.archetype.tags() as *const Tags as *mut Tags) };
            world_tag_storages.push(
                tags.get_mut(*tag_type)
                    .expect("tag storage not present when deserializing"),
            );
        }
        let num_world_values = world_tag_storages.iter().map(|ts| ts.len()).nth(0);
        let num_tag_values = deserialized_tags
            .iter()
            .map(|ts| ts.len())
            .nth(0)
            .unwrap_or(0);
        for i in 0..num_tag_values {
            let mut matching_idx = None;
            if let Some(num_world_values) = num_world_values {
                for j in 0..num_world_values {
                    let mut is_matching = true;
                    for tag_idx in 0..tag_types.len() {
                        unsafe {
                            let (de_ptr, stride, _) = deserialized_tags[tag_idx].data_raw();
                            let (world_ptr, _, _) = world_tag_storages[tag_idx].data_raw();
                            let (tag_type, tag_meta) = tag_types[tag_idx];
                            let de_offset = (i * stride) as isize;
                            let world_offset = (j * stride) as isize;
                            if !tag_meta.equals(
                                de_ptr.as_ptr().offset(de_offset),
                                world_ptr.as_ptr().offset(world_offset),
                            ) {
                                is_matching = false;
                                break;
                            }
                        }
                    }
                    if is_matching {
                        matching_idx = Some(j);
                        break;
                    }
                }
            }
            // If we have a matching tag set, we will drop our temporary values manually.
            // The temporary TagStorages in `deserialized_tags` will be forgotten because
            // we may be moving data into the existing World.
            if let Some(world_idx) = matching_idx {
                chunkset_map.insert(i, world_idx);
                for tag_idx in 0..tag_types.len() {
                    unsafe {
                        let (tag_type, tag_meta) = tag_types[tag_idx];
                        let (de_ptr, stride, _) = deserialized_tags[tag_idx].data_raw();
                        let de_offset = (i * stride) as isize;
                        tag_meta.drop(de_ptr.as_ptr().offset(de_offset) as *mut u8);
                    }
                }
            } else {
                let chunkset_idx = this.archetype.alloc_chunk_set(|tags| {
                    for tag_idx in 0..tag_types.len() {
                        unsafe {
                            let (tag_type, tag_meta) = tag_types[tag_idx];
                            let (de_ptr, stride, _) = deserialized_tags[tag_idx].data_raw();
                            let de_offset = (i * stride) as isize;
                            let mut world_storage = tags
                                .get_mut(tag_type)
                                .expect("expected tag storage when allocating chunk set");
                            world_storage.push_raw(de_ptr.as_ptr().offset(de_offset));
                        }
                    }
                });
                chunkset_map.insert(i, chunkset_idx);
            }
        }
        for tag in deserialized_tags.drain(0..) {
            tag.forget_data();
        }
        if num_tag_values == 0 {
            dbg!(tag_types.iter().map(|(ty, _)| ty).collect::<Vec<_>>());
            let chunkset_idx = this.archetype.alloc_chunk_set(|_| {});
            chunkset_map.insert(0, chunkset_idx);
        }
        dbg!(&chunkset_map);
        Ok(chunkset_map)
    }
}

impl<'de, 'a, 'b, WD: WorldDeserializer> Visitor<'de> for TagsDeserializer<'a, 'b, WD> {
    type Value = (Vec<TagStorage>, Self);

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("sequence of objects")
    }
    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        let tag_types = self.archetype.description().tags();
        let mut deserialized_tags = Vec::new();
        for idx in 0..tag_types.len() {
            let (tag_type, tag_meta) = tag_types[idx];
            let mut tag_storage = TagStorage::new(tag_meta);
            if let None = seq.next_element_seed(TagStorageDeserializer {
                user: self.user,
                tag_storage: &mut tag_storage,
                tag_type: &tag_type,
                tag_meta: &tag_meta,
            })? {
                break;
            }
            deserialized_tags.push(tag_storage);
        }
        Ok((deserialized_tags, self))
    }
}

struct TagStorageDeserializer<'a, 'b, WD: WorldDeserializer> {
    user: &'b WD,
    tag_storage: &'a mut TagStorage,
    tag_type: &'a TagTypeId,
    tag_meta: &'a TagMeta,
}
impl<'de, 'a, 'b, WD: WorldDeserializer> DeserializeSeed<'de>
    for TagStorageDeserializer<'a, 'b, WD>
{
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        println!("user deserialize tag {:?}", self.tag_type);
        self.user
            .deserialize_tags(deserializer, self.tag_type, self.tag_meta, self.tag_storage)?;
        Ok(())
    }
}

struct ChunkSetDeserializer<'a, 'b, WD: WorldDeserializer> {
    user: &'b WD,
    world: &'a mut World,
    archetype_idx: usize,
    chunkset_map: &'a ChunkSetMapping,
}
impl<'de, 'a, 'b, WD: WorldDeserializer> DeserializeSeed<'de> for ChunkSetDeserializer<'a, 'b, WD> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(self)
    }
}

impl<'de, 'a, 'b, WD: WorldDeserializer> Visitor<'de> for ChunkSetDeserializer<'a, 'b, WD> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("sequence of objects")
    }
    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        for idx in 0.. {
            let chunkset_idx = self.chunkset_map.get(&idx).cloned();
            if let None = seq.next_element_seed(ChunkListDeserializer {
                user: self.user,
                world: self.world,
                archetype_idx: self.archetype_idx,
                chunkset_idx: chunkset_idx,
            })? {
                break;
            }
        }
        Ok(())
    }
}

struct ChunkListDeserializer<'a, 'b, WD: WorldDeserializer> {
    user: &'b WD,
    world: &'a mut World,
    archetype_idx: usize,
    chunkset_idx: Option<usize>,
}
impl<'de, 'a, 'b, WD: WorldDeserializer> DeserializeSeed<'de>
    for ChunkListDeserializer<'a, 'b, WD>
{
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(self)
    }
}

impl<'de, 'a, 'b, WD: WorldDeserializer> Visitor<'de> for ChunkListDeserializer<'a, 'b, WD> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("sequence of struct Chunk")
    }
    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        loop {
            if let None = seq.next_element_seed(ChunkDeserializer {
                user: self.user,
                world: self.world,
                archetype_idx: self.archetype_idx,
                chunkset_idx: self.chunkset_idx.expect("expected chunkset_idx"),
            })? {
                break;
            }
        }
        Ok(())
    }
}

#[derive(Deserialize, Debug)]
#[serde(field_identifier, rename_all = "lowercase")]
enum ChunkField {
    Entities,
    Components,
}
struct ChunkDeserializer<'a, 'b, WD: WorldDeserializer> {
    user: &'b WD,
    world: &'a mut World,
    archetype_idx: usize,
    chunkset_idx: usize,
}
impl<'de, 'a, 'b, WD: WorldDeserializer> DeserializeSeed<'de> for ChunkDeserializer<'a, 'b, WD> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_struct("Chunk", &["entities", "components"], self)
    }
}

impl<'de, 'a, 'b, WD: WorldDeserializer> Visitor<'de> for ChunkDeserializer<'a, 'b, WD> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("struct Chunk")
    }

    fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
    where
        V: de::MapAccess<'de>,
    {
        let mut chunk_ranges = None;
        while let Some(key) = map.next_key()? {
            match key {
                ChunkField::Entities => {
                    println!("entities");
                    chunk_ranges = Some(map.next_value_seed(EntitiesDeserializer {
                        user: self.user,
                        world: self.world,
                        archetype_idx: self.archetype_idx,
                        chunkset_idx: self.chunkset_idx,
                    })?);
                }
                ChunkField::Components => {
                    Some(
                        map.next_value_seed(ComponentsDeserializer {
                            user: self.user,
                            world: self.world,
                            archetype_idx: self.archetype_idx,
                            chunkset_idx: self.chunkset_idx,
                            chunk_ranges: chunk_ranges
                                .as_ref()
                                .expect("expected entities before components"),
                        })?,
                    );
                }
            }
        }
        // // TODO fix mutability issue in the storage API here?
        // let tags = unsafe { &mut *(self.archetype.tags() as *const Tags as *mut Tags) };
        // let mut idx = 0;
        // loop {
        //     let chunk_set = self.world.find_or_create_chunk(self.archetype_idx, tags);

        //     let (tag_type, tag_meta) = tag_types[idx];
        //     let tag_storage = tags
        //         .get_mut(tag_type)
        //         .expect("tag storage not present when deserializing");
        //     if let None = seq.next_element_seed(TagStorageDeserializer {
        //         user: self.user,
        //         tag_storage,
        //         tag_type: &tag_type,
        //         tag_meta: &tag_meta,
        //     })? {
        //         break;
        //     }
        //     idx += 1;
        // }
        Ok(())
    }
}

struct EntitiesDeserializer<'a, 'b, WD: WorldDeserializer> {
    user: &'b WD,
    world: &'a mut World,
    archetype_idx: usize,
    chunkset_idx: usize,
}
impl<'de, 'a, 'b, WD: WorldDeserializer> DeserializeSeed<'de> for EntitiesDeserializer<'a, 'b, WD> {
    type Value = Vec<(usize, usize)>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let mut entities = Vec::new();
        self.user.deserialize_entities(
            deserializer,
            &mut self.world.entity_allocator,
            &mut entities,
        );
        let mut archetype = &mut self.world.storage_mut().archetypes_mut()[self.archetype_idx];
        let mut chunk_ranges = Vec::new();
        let mut chunk_idx = archetype.get_free_chunk(self.chunkset_idx, entities.len());
        let mut entities_in_chunk = 0;
        for entity in entities {
            let mut chunk = {
                let mut chunkset = &mut archetype.chunksets_mut()[self.chunkset_idx];
                dbg!(chunk_idx);
                let mut chunk = &mut chunkset[chunk_idx];
                if chunk.is_full() {
                    chunk_ranges.push((chunk_idx, entities_in_chunk));
                    chunk_idx = archetype.get_free_chunk(self.chunkset_idx, 1);
                    let mut chunkset = &mut archetype.chunksets_mut()[self.chunkset_idx];
                    &mut chunkset[chunk_idx]
                } else {
                    chunk
                }
            };
            chunk.write().0.push(entity);
            entities_in_chunk += 1;
        }
        if entities_in_chunk > 0 {
            chunk_ranges.push((chunk_idx, entities_in_chunk));
        }
        Ok((chunk_ranges))
    }
}
struct ComponentsDeserializer<'a, 'b, WD: WorldDeserializer> {
    user: &'b WD,
    world: &'a mut World,
    archetype_idx: usize,
    chunkset_idx: usize,
    chunk_ranges: &'a Vec<(usize, usize)>,
}
impl<'de, 'a, 'b, WD: WorldDeserializer> DeserializeSeed<'de>
    for ComponentsDeserializer<'a, 'b, WD>
{
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(self)
    }
}

impl<'de, 'a, 'b, WD: WorldDeserializer> Visitor<'de> for ComponentsDeserializer<'a, 'b, WD> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("sequence of objects")
    }
    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        let mut archetype = &mut self.world.storage_mut().archetypes_mut()[self.archetype_idx];
        for idx in 0..archetype.description().components().len() {
            let desc = archetype.description();
            let (comp_type, comp_meta) = desc.components()[idx];
            let mut chunkset = &mut archetype.chunksets_mut()[self.chunkset_idx];
            if let None = seq.next_element_seed(ComponentDataDeserializer {
                user: self.user,
                comp_type: &comp_type,
                comp_meta: &comp_meta,
                chunkset: &mut chunkset,
                chunk_ranges: self.chunk_ranges,
            })? {
                break;
            }
        }
        Ok(())
    }
}

struct ComponentDataDeserializer<'a, 'b, WD: WorldDeserializer> {
    user: &'b WD,
    comp_type: &'a ComponentTypeId,
    comp_meta: &'a ComponentMeta,
    chunkset: &'a mut Chunkset,
    chunk_ranges: &'a Vec<(usize, usize)>,
}
impl<'de, 'a, 'b, WD: WorldDeserializer> DeserializeSeed<'de>
    for ComponentDataDeserializer<'a, 'b, WD>
{
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let mut components_written_in_range = 0;
        let mut range_idx = 0;
        self.user.deserialize_components(
            deserializer,
            self.comp_type,
            self.comp_meta,
            &mut |ptr: NonNull<u8>, len| {
                let mut written = 0;
                while written < len {
                    dbg!(written);
                    dbg!(len);
                    dbg!(range_idx);
                    let chunk_range = self.chunk_ranges[range_idx];
                    let mut chunk = &mut self.chunkset[chunk_range.0];
                    let copy_in_range =
                        std::cmp::min(len - written, chunk_range.1 - components_written_in_range);
                    unsafe {
                        let mut comp_storage = (&mut *chunk.write().1.get())
                            .get_mut(*self.comp_type)
                            .expect(
                                "expected ComponentResourceSet when deserializing component data",
                            );
                        comp_storage.writer().push_raw(
                            NonNull::new_unchecked(
                                ptr.as_ptr()
                                    .offset((self.comp_meta.size() * written) as isize),
                            ),
                            copy_in_range,
                        );
                    }
                    components_written_in_range += copy_in_range;
                    if chunk_range.1 <= components_written_in_range {
                        range_idx += 1;
                        components_written_in_range = 0;
                    }
                    written += copy_in_range;
                }
            },
        )?;
        println!("deserialized component!!");
        Ok(())
    }
}
