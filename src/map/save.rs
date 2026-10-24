//! A map in a save file (`crate::save`): the site as `map.pb`, and the
//! endless world round it as `chunks/NNNN.pb`.
//!
//! The site is stored whole, as a map file would hold it, so a save does not
//! depend on the map file it was played from still being there, or still
//! being the same. Names, not ids, as in a map file ([`super::format`]): a
//! tile, a grid layer, an object layer and a prop are all looked up by name
//! on the way back in, and a name this build does not know is refused.
//!
//! Round it, every chunk the world knows anything about: its ground, stored
//! verbatim, if it is loaded; and what has been taken from its heaps of
//! scrap, whether it is loaded or not. A chunk's ground is a pure function of
//! the seed, so what is stored is what the generator would give — kept all
//! the same, and stamped with the generator's version
//! ([`super::wasteland::GENERATION`]), so the save says what the world
//! looked like rather than how to work it out.

use std::collections::BTreeMap;

use crate::save::{invalid, pb, Invalid};

use super::chunk::{Chunk, ChunkPos, CHUNK_AREA};
use super::wasteland::{self, Scrap, Wasteland, MAX_SCRAP, SCRAP_VARIANTS};
use super::{GridLayer, Map, Object, ObjectKind, ObjectLayer, Point, Size, TerrainId, BASE};

/// The names `tiles` index into, in the order they are first met — so a map
/// saved twice is saved the same.
struct Palette {
    names: Vec<String>,
    ids: Vec<TerrainId>,
}

impl Palette {
    fn new() -> Palette {
        Palette { names: Vec::new(), ids: Vec::new() }
    }

    fn index(&mut self, tile: TerrainId) -> u32 {
        match self.ids.iter().position(|&known| known == tile) {
            Some(index) => index as u32,
            None => {
                self.ids.push(tile);
                self.names.push(tile.name().to_string());
                (self.ids.len() - 1) as u32
            }
        }
    }
}

/// The tiles a saved palette names, refusing a name this build does not know.
fn resolve(names: &[String]) -> Result<Vec<TerrainId>, Invalid> {
    names
        .iter()
        .map(|name| TerrainId::from_name(name).ok_or_else(|| Invalid::new(format!("no such terrain {name:?}"))))
        .collect()
}

fn tile(palette: &[TerrainId], index: u32) -> Result<TerrainId, Invalid> {
    palette
        .get(index as usize)
        .copied()
        .ok_or_else(|| Invalid::new(format!("terrain {index} of a palette of {}", palette.len())))
}

impl Map {
    /// The site, everything a map file would hold.
    pub fn save_site(&self) -> pb::SiteMap {
        let mut palette = Palette::new();
        let terrain_layers = self
            .terrain
            .iter()
            .map(|layer| pb::TerrainLayer { tiles: layer.tiles.iter().map(|&tile| palette.index(tile)).collect() })
            .collect();
        pb::SiteMap {
            initial_stock: self.initial_stock.iter().map(|stock| pb::Bin { cell: Some(stock.cell.into()), stored: stock.pieces }).collect(),
            width: self.size.width,
            height: self.size.height,
            terrain_names: palette.names,
            terrain_layers,
            grids: GridLayer::ALL
                .into_iter()
                .filter(|&layer| self.grid_count(layer) > 0)
                .map(|layer| pb::Grid { layer: layer.name().to_string(), cells: self.grids[layer as usize].clone() })
                .collect(),
            objects: ObjectLayer::ALL
                .into_iter()
                .filter(|&layer| !self.objects(layer).is_empty())
                .map(|layer| {
                    let mut kinds: Vec<String> = Vec::new();
                    let mut kind = Vec::new();
                    for object in self.objects(layer) {
                        let index = match kinds.iter().position(|known| known == object.kind.as_str()) {
                            Some(index) => index,
                            None => {
                                kinds.push(object.kind.as_str().to_string());
                                kinds.len() - 1
                            }
                        };
                        kind.push(index as u32);
                    }
                    pb::ObjectLayer {
                        layer: layer.name().to_string(),
                        kinds,
                        kind,
                        at: pb::pack(self.objects(layer).iter().map(|object| object.at)),
                    }
                })
                .collect(),
            wasteland: self.wasteland.map(|land| pb::Wasteland { seed: land.seed() }),
        }
    }

    /// The site as saved, built the way a map file is read back
    /// ([`Map::from_json`]): the ground, then what is round it, then the
    /// grids, then the objects — and passability worked out from what was
    /// stored rather than trusted.
    pub fn restore_site(saved: &pb::SiteMap) -> Result<Map, Invalid> {
        let size = Size::try_new(saved.width, saved.height)
            .ok_or_else(|| Invalid::new(format!("a {}x{} map", saved.width, saved.height)))?;
        let palette = resolve(&saved.terrain_names)?;
        let [base] = saved.terrain_layers.as_slice() else {
            return invalid(format!("{} terrain layers, where a map has one", saved.terrain_layers.len()));
        };
        if base.tiles.len() != size.area() {
            return invalid(format!("{} tiles for a {}x{} map", base.tiles.len(), size.width, size.height));
        }
        let tiles = base.tiles.iter().map(|&index| tile(&palette, index)).collect::<Result<Vec<_>, _>>()?;

        let mut map = Map::from_base_layer(size, tiles);
        debug_assert_eq!(map.terrain.len(), BASE + 1);
        map.set_wasteland(saved.wasteland.map(|land| Wasteland::new(land.seed)));
        for grid in &saved.grids {
            let layer = GridLayer::from_name(&grid.layer)
                .ok_or_else(|| Invalid::new(format!("no such grid layer {:?}", grid.layer)))?;
            if grid.cells.len() != size.area() {
                return invalid(format!("{} {} cells for a map of {}", grid.cells.len(), layer.name(), size.area()));
            }
            for (index, &value) in grid.cells.iter().enumerate() {
                let at = Point::new(index as i32 % size.width, index as i32 / size.width);
                if !map.set_grid(layer, at, value) {
                    return invalid(format!("{} is {value} at {}, {}", layer.name(), at.x, at.y));
                }
            }
        }
        for objects in &saved.objects {
            let layer = ObjectLayer::from_name(&objects.layer)
                .ok_or_else(|| Invalid::new(format!("no such object layer {:?}", objects.layer)))?;
            let at = pb::unpack(&objects.at, layer.name())?;
            if at.len() != objects.kind.len() {
                return invalid(format!("{} {} placed and {} named", at.len(), layer.name(), objects.kind.len()));
            }
            for (&kind, at) in objects.kind.iter().zip(at) {
                let name = objects
                    .kinds
                    .get(kind as usize)
                    .ok_or_else(|| Invalid::new(format!("{} {kind} of {}", layer.name(), objects.kinds.len())))?;
                map.add_object(layer, Object { at, kind: ObjectKind::new(name.clone()) });
            }
        }
        let mut stocked = std::collections::BTreeSet::new();
        for stock in &saved.initial_stock {
            let cell = pb::cell(stock.cell, "initial stock")?;
            if !stocked.insert(cell) || !map.set_initial_stock(cell, stock.stored) {
                return invalid(format!("invalid or duplicate initial stock at {}, {}", cell.x, cell.y));
            }
        }
        Ok(map)
    }

    /// Every chunk the world knows about, in files of up to `per_file`: the
    /// loaded ones with their ground, and every one with a heap that has been
    /// salvaged. In order of position, so a world saved twice is saved the
    /// same, whatever order its chunks were loaded in.
    pub fn save_chunks(&self, per_file: usize) -> Vec<pb::Chunks> {
        let mut records: BTreeMap<ChunkPos, pb::ChunkRecord> = BTreeMap::new();
        for (&(pos, index), &taken) in &self.salvaged {
            records
                .entry(pos)
                .or_insert_with(|| pb::ChunkRecord { x: pos.x, y: pos.y, ..Default::default() })
                .salvaged
                .push(pb::Tally { index: index as u32, taken: taken as u32 });
        }
        let mut loaded: Vec<(ChunkPos, &Chunk)> = self.chunks.iter().map(|(pos, chunk)| (pos, &**chunk)).collect();
        loaded.sort_unstable_by_key(|&(pos, _)| pos);
        let mut grounds: BTreeMap<ChunkPos, &Chunk> = BTreeMap::new();
        for (pos, chunk) in loaded {
            records.entry(pos).or_insert_with(|| pb::ChunkRecord { x: pos.x, y: pos.y, ..Default::default() });
            grounds.insert(pos, chunk);
        }

        let records: Vec<(ChunkPos, pb::ChunkRecord)> = records.into_iter().collect();
        records
            .chunks(per_file.max(1))
            .map(|batch| {
                let mut palette = Palette::new();
                let chunks = batch
                    .iter()
                    .map(|(pos, record)| {
                        let mut record = record.clone();
                        record.ground = grounds.get(pos).map(|chunk| pb::Ground {
                            tiles: chunk.tiles().iter().map(|&tile| palette.index(tile)).collect(),
                            heaps: chunk
                                .found()
                                .iter()
                                .map(|&(index, heap)| pb::Heap {
                                    index: index as u32,
                                    variant: heap.variant as u32,
                                    pieces: heap.pieces as u32,
                                })
                                .collect(),
                        });
                        record
                    })
                    .collect();
                pb::Chunks { generation: wasteland::GENERATION, terrain_names: palette.names, chunks }
            })
            .collect()
    }

    /// Put back every chunk a save knew about: what was taken from its heaps,
    /// and its ground, if it was loaded. A chunk off a map with nothing round
    /// it, or one that is all site, is refused — no world would have had it.
    pub fn restore_chunks<'a>(&mut self, files: impl IntoIterator<Item = &'a pb::Chunks>) -> Result<(), Invalid> {
        let site = self.site();
        let mut grounds = Vec::new();
        for file in files {
            if file.generation > wasteland::GENERATION {
                return invalid(format!(
                    "ground from generator {}, newer than this one ({})",
                    file.generation,
                    wasteland::GENERATION
                ));
            }
            let palette = resolve(&file.terrain_names)?;
            for record in &file.chunks {
                let pos = ChunkPos::new(record.x, record.y);
                let place = || format!("chunk {}, {}", pos.x, pos.y);
                if self.wasteland.is_none() || super::chunk_is_all_site(pos, site) {
                    return invalid(format!("{} is not in the wasteland", place()));
                }
                for tally in &record.salvaged {
                    let (Ok(index), Ok(taken)) = (u16::try_from(tally.index), u8::try_from(tally.taken)) else {
                        return invalid(format!("{}: {} taken from heap {}", place(), tally.taken, tally.index));
                    };
                    if index as usize >= CHUNK_AREA || taken == 0 {
                        return invalid(format!("{}: {taken} taken from heap {index}", place()));
                    }
                    if self.salvaged.insert((pos, index), taken).is_some() {
                        return invalid(format!("{}: heap {index} twice", place()));
                    }
                }
                if let Some(ground) = &record.ground {
                    grounds.push((pos, restore_ground(ground, &palette).map_err(|error| error.within(place()))?));
                }
            }
        }
        for (pos, chunk) in grounds {
            if self.chunks.contains(pos) {
                return invalid(format!("chunk {}, {} twice", pos.x, pos.y));
            }
            self.chunks.get_or_insert_with(pos, || chunk);
            self.clear_salvaged(pos);
        }
        self.salvage_revision = self.salvage_revision.wrapping_add(1);
        Ok(())
    }
}

/// A chunk's ground as stored: every tile, and its heaps as they were found.
fn restore_ground(ground: &pb::Ground, palette: &[TerrainId]) -> Result<Box<Chunk>, Invalid> {
    if ground.tiles.len() != CHUNK_AREA {
        return invalid(format!("{} tiles in a chunk of {CHUNK_AREA}", ground.tiles.len()));
    }
    let mut tiles = [TerrainId(0); CHUNK_AREA];
    for (slot, &index) in tiles.iter_mut().zip(&ground.tiles) {
        *slot = tile(palette, index)?;
    }
    let mut heaps = Vec::with_capacity(ground.heaps.len());
    for heap in &ground.heaps {
        let index = heap.index as usize;
        let in_order = heaps.last().is_none_or(|&(last, _): &(u16, Scrap)| (last as usize) < index);
        if index >= CHUNK_AREA || !in_order || !tiles[index].is_passable() {
            return invalid(format!("a heap of scrap at {index}"));
        }
        if heap.variant >= SCRAP_VARIANTS as u32 || !(1..=MAX_SCRAP as u32).contains(&heap.pieces) {
            return invalid(format!("a heap of {} pieces drawn as {}", heap.pieces, heap.variant));
        }
        heaps.push((index as u16, Scrap { variant: heap.variant as u8, pieces: heap.pieces as u8 }));
    }
    Ok(Chunk::from_parts(tiles, heaps))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Bounds, FLOOR, WALL};

    /// A small site in the wasteland of seed 3, which has a scrapyard in
    /// reach, with something on it.
    fn site() -> Map {
        let mut map = Map::new(Size::new(10, 8), FLOOR).with_wasteland(3);
        map.set_terrain(Point::new(4, 4), WALL);
        crate::sim::testing::prop_at(&mut map, "fridge", Point::new(2, 2));
        map.set_ceiling(Point::new(1, 1), true);
        map
    }

    #[test]
    fn a_site_comes_back_as_it_was_saved() {
        let map = site();
        let restored = Map::restore_site(&map.save_site()).unwrap();
        assert_eq!(restored.to_json().unwrap(), map.to_json().unwrap());
        assert_eq!(restored.save_site(), map.save_site());
    }

    #[test]
    fn stored_chunks_are_the_ground_the_generator_gives_with_what_was_taken_gone() {
        let mut map = site();
        let mut heaps = Vec::new();
        map.heaps(Bounds::new(Point::new(-150, -150), Size::new(300, 300)), |cell, _| heaps.push(cell));
        assert!(!heaps.is_empty(), "a scrapyard in reach");
        let heap = heaps[0];
        map.load_chunks(Bounds::from_corners(heap.offset(-40, -40), heap.offset(40, 40)));
        while map.take_scrap(heap).is_some() {}
        assert!(map.scrap(heap).is_none() && map.is_passable(heap), "the heap is gone");

        let mut restored = Map::restore_site(&map.save_site()).unwrap();
        restored.restore_chunks(&map.save_chunks(7)).unwrap();
        assert_eq!(restored.loaded_chunks(), map.loaded_chunks());
        for (pos, chunk) in map.chunks.iter() {
            let back = restored.chunks.get(pos).expect("every loaded chunk comes back loaded");
            assert_eq!(back.tiles(), chunk.tiles());
            assert_eq!(back.found(), chunk.found());
            for cell in pos.bounds().points() {
                assert_eq!(back.is_passable(cell), chunk.is_passable(cell), "{cell:?}");
                assert_eq!(back.is_see_through(cell), chunk.is_see_through(cell), "{cell:?}");
            }
        }
        assert!(restored.scrap(heap).is_none() && restored.is_passable(heap), "the emptied heap stays empty");
        // A chunk let go of and loaded again is the same ground.
        let far = ChunkPos::of(heap);
        restored.unload_chunks(|pos| pos != far);
        restored.load_chunk(far);
        assert!(restored.scrap(heap).is_none());
    }

    #[test]
    fn a_chunk_on_the_site_or_off_a_map_with_nothing_round_it_is_refused() {
        let map = site();
        let mut chunks = map.save_chunks(10);
        chunks.push(pb::Chunks {
            generation: wasteland::GENERATION,
            terrain_names: Vec::new(),
            chunks: vec![pb::ChunkRecord { x: 0, y: 0, ..Default::default() }],
        });
        let mut small = Map::new(Size::new(64, 64), FLOOR).with_wasteland(3);
        assert!(small.restore_chunks(&chunks[chunks.len() - 1..]).is_err(), "all site");
        let mut bounded = Map::new(Size::new(4, 4), FLOOR);
        let off = pb::Chunks {
            generation: wasteland::GENERATION,
            terrain_names: Vec::new(),
            chunks: vec![pb::ChunkRecord { x: 5, y: 5, ..Default::default() }],
        };
        assert!(bounded.restore_chunks([&off]).is_err(), "nothing round it");
    }

    #[test]
    fn ground_from_a_newer_generator_is_refused() {
        let mut map = site();
        map.load_chunks(map.navigation_area());
        let mut chunks = map.save_chunks(100);
        chunks[0].generation = wasteland::GENERATION + 1;
        assert!(site().restore_chunks(&chunks).is_err());
    }
}
