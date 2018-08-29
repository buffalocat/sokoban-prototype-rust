extern crate sdl2;

use sdl2::pixels::Color;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::rect::Rect;
use sdl2::render::WindowCanvas;

use std::time::Duration;
use std::collections::{HashMap, HashSet, VecDeque};
use std::mem;

use std::sync::atomic::{AtomicUsize, Ordering};
static ID_COUNT: AtomicUsize = AtomicUsize::new(1);

fn new_id() -> usize {
    let id = ID_COUNT.fetch_add(1, Ordering::SeqCst);
    if id == 0 {
        panic!("You created too many billions of objects while playing my game! Thank you!");
    }
    return id;
}

const SCREEN_WIDTH: u32 = 800;
const SCREEN_HEIGHT: u32 = 600;

const ANIMATION_LENGTH: u32 = 6;
const UNDO_COOLDOWN_MAX: u32 = 6;

const MESH: i32 = 40;

/// Abstract Type for "things that live in the world map"
/// It is always implemented indirectly, via Layers.
/// Every game object implements exactly one Layer type.
trait GameObject {
    // We can't include it because the return type has indeterminate size,
    // but in spirit every game object should have a constructor function!
    fn get_id(&self) -> usize;
    fn get_pos(&self) -> (i32, i32);
    fn get_layer(&self) -> Layer;
    fn pushable(&self) -> bool;
    fn shift_pos(&mut self, (i32, i32), &mut DeltaFrame);
    fn set_pos(&mut self, (i32, i32));
    fn draw(&self, &mut WindowCanvas);
}

impl std::fmt::Debug for GameObject {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Object at {:?}", self as *const GameObject)
    }
}

/// Keeps track of whether the game is ready to receive new input
/// A state of Anim(n) indicates there are n frames of animation left
/// This simple model only makes sense for a discrete-time puzzle game
enum AnimationState {
    Ready,
    Wait(u32),
}

struct Player {
    id: usize,
    x: i32,
    y: i32,
    color: Color,
}

impl Player {
    fn new(x: i32, y: i32) -> Player {
        Player {
            id: new_id(),
            x,
            y,
            color: Color::RGB(230, 240, 200),
        }
    }
}

impl GameObject for Player {
    fn get_id(&self) -> usize {
        self.id
    }

    fn get_layer(&self) -> Layer {
        Layer::Solid
    }
    
    fn get_pos(&self) -> (i32, i32) {
        (self.x, self.y)
    }
    
    fn pushable(&self) -> bool {
        true
    }
    
    fn shift_pos(&mut self, (dx, dy): (i32, i32), delta_frame: &mut DeltaFrame) {
        self.x += dx;
        self.y += dy;
        delta_frame.push(Box::new(MotionDelta {
            id: self.id,
            x: self.x,
            y: self.y,
            layer: self.get_layer(),
            dx,
            dy,
        }));
        //println!("Player moved from {:?} to {:?}", (self.x,self.y), (self.x+dx,self.y+dy)); 
    }
    
    fn set_pos(&mut self, (x, y): (i32, i32)) {
        self.x = x;
        self.y = y;
    }
    
    fn draw(&self, canvas: &mut WindowCanvas) {
        canvas.set_draw_color(self.color);
        canvas.fill_rect(Rect::new(MESH*self.x, MESH*self.y, MESH as u32, MESH as u32)).expect("Failed to draw Player rect");
    }
}

struct Block {
    id: usize,
    x: i32,
    y: i32,
    pushable: bool,
    color: Color,
}

impl Block {
    fn new_block(x: i32, y: i32) -> Block {
        Block {
            id: new_id(),
            x,
            y,
            pushable: true,
            color: Color::RGB(200, 180, 100),
        }
    }
    
    fn new_wall(x: i32, y: i32) -> Block {
        Block {
            id: new_id(),
            x,
            y,
            pushable: false,
            color: Color::RGB(80, 20, 50),
        }
    }
}

impl GameObject for Block {
    fn get_id(&self) -> usize {
        self.id
    }

    fn get_layer(&self) -> Layer {
        Layer::Solid
    }

    fn get_pos(&self) -> (i32, i32) {
        (self.x, self.y)
    }

    fn pushable(&self) -> bool {
        self.pushable
    }
    
    fn shift_pos(&mut self, (dx, dy): (i32, i32), delta_frame: &mut DeltaFrame) {
        self.x += dx;
        self.y += dy;
        delta_frame.push(Box::new(MotionDelta {
            id: self.id,
            x: self.x,
            y: self.y,
            layer: self.get_layer(),
            dx,
            dy,
        }));
        //println!("Block moved from {:?} to {:?}", (self.x,self.y), (self.x+dx,self.y+dy)); 
    }
    
    fn set_pos(&mut self, (x, y): (i32, i32)) {
        self.x = x;
        self.y = y;
    }
    
    fn draw(&self, canvas: &mut WindowCanvas) {
        canvas.set_draw_color(self.color);
        canvas.fill_rect(Rect::new(MESH*self.x, MESH*self.y, MESH as u32, MESH as u32)).expect("Failed to draw Player rect");
    }
}

/// Abstraction of "Undoable Actions"
/// Deltas are created automatically, placed on a stack, and then reverted when you undo
trait Delta {
    fn revert(&mut self, &mut WorldMap);
}

/// Store the current (post-move) location of an object
struct MotionDelta {
    id: usize,
    x: i32,
    y: i32,
    layer: Layer,
    dx: i32,
    dy: i32,
}

impl Delta for MotionDelta {
    fn revert(&mut self, map: &mut WorldMap) {
        // For now, redo is a dummy frame
        let mut redo = DeltaFrame::new();
        let mut object = map.take_id(self.x, self.y, &self.layer, self.id).unwrap();
        object.shift_pos((-self.dx, -self.dy), &mut redo);
        map.put_quiet(object);
    }
}

/// Move ownership of object from game map to the undo stack
struct DeletionDelta {
    object: Option<Box<dyn GameObject>>,
}

impl DeletionDelta {
    fn new(object: Box<dyn GameObject>) -> DeletionDelta {
        DeletionDelta {
            object: Some(object),
        }
    }
}

// TODO: Use a redo stack, and call .put() instead!
impl Delta for DeletionDelta {
    fn revert(&mut self, map: &mut WorldMap) {
        if let Some(object) = mem::replace(&mut self.object, None) {
            map.put_quiet(object);
        }
    }
}

struct CreationDelta {
    id: usize,
    pos: (i32, i32),
    layer: Layer,
}

impl CreationDelta {
    fn new(object: &Box<dyn GameObject>) -> CreationDelta {
        CreationDelta {
            id: object.get_id(),
            pos: object.get_pos(),
            layer: object.get_layer(),
        }
    }
}

impl Delta for CreationDelta {
    fn revert(&mut self, map: &mut WorldMap) {
        let (x, y) = self.pos;
        map.take(x, y, &self.layer);
    }
}

/// Collection of Deltas representing changes in one step of game logic
struct DeltaFrame{
    deltas: Vec<Box<dyn Delta>>,
}

impl DeltaFrame {
    fn new() -> DeltaFrame {
        DeltaFrame {
            deltas: vec!(),
        }
    }

    fn revert(&mut self, map: &mut WorldMap) {
        for delta in self.deltas.iter_mut() {
            delta.revert(map);
        }
    }
    
    fn push(&mut self, delta: Box<dyn Delta>) {
        self.deltas.push(delta);
    }
    
    fn trivial(&self) -> bool {
        self.deltas.is_empty()
    }
}

struct UndoStack {
    stack: VecDeque<DeltaFrame>,
    max_depth: usize,
    size: usize,
}

impl UndoStack {
    fn new(max_depth: usize) -> UndoStack{
        UndoStack {
            stack: VecDeque::with_capacity(max_depth),
            max_depth,
            size: 0,
        }
    }
    
    fn push(&mut self, delta: DeltaFrame) {
        if self.size == self.max_depth {
            self.stack.pop_back();
            self.stack.push_front(delta);
        } else {
            self.stack.push_front(delta);
            self.size += 1;
        }
    }
    
    fn pop(&mut self, map: &mut WorldMap) {
        if self.size > 0 {
            self.stack.pop_front().unwrap().revert(map);
            self.size -= 1;
        }
    }
}

enum Layer {
    Solid,
    Player,
    Floor,
}

// Treat Layer as an integer valued enum
impl Layer {
    fn index(layer: &Layer) -> usize {
        match layer {
            Layer::Floor => 0,
            Layer::Player => 1,
            Layer::Solid => 2,
        }
    }
}

const NUMBER_OF_LAYERS: usize = 3;

struct MapCell {
    layers: [Vec<Box<dyn GameObject>>; NUMBER_OF_LAYERS],
}

impl MapCell {
    // This code is weirdly fragile if we want to change the number of layers!
    // Fortunately it's an easy fix, and that shouldn't change often.
    fn new() -> MapCell {
        MapCell {
            layers: [vec!(), vec!(), vec!()],
        }
    }
    
    fn draw(&self, canvas: &mut WindowCanvas) {
        for layer in self.layers.iter() {
            for object in layer.iter() {
                object.draw(canvas);
            }
        }
    }
    
    // Mutably borrow the top object of a layer
    fn view(&mut self, layer: &Layer) -> Option<&mut Box<dyn GameObject>> {
        self.layers[Layer::index(layer)].last_mut()
    }
        
    // Take the top object of a layer and put it in a deletion delta
    fn delete(&mut self, layer: &Layer, delta: &mut DeltaFrame) -> bool {
        match self.layers[Layer::index(layer)].pop() {
            Some(object) => {
                delta.push(Box::new(DeletionDelta::new(object)));
                true
            },
            None => false,
        }
    }
    
    // Delete a specific object (if found)
    fn delete_id(&mut self, layer: &Layer, id: usize, delta: &mut DeltaFrame) -> bool {
        let mut found = false;
        let mut index = 0;
        for (i, object) in self.layers[Layer::index(layer)].iter().enumerate() {
            if object.get_id() == id {
                found = true;
                index = i;
                break;
            }
        }
        if found {
            delta.push(Box::new(DeletionDelta::new(self.layers[Layer::index(layer)].remove(index))));
        }
        found
    }
    
    // Take the top object of a layer and return it
    fn take(&mut self, layer: &Layer) -> Option<Box<dyn GameObject>> {
        self.layers[Layer::index(layer)].pop()
    }
    
    // Take a specific object (if found)
    fn take_id(&mut self, layer: &Layer, id: usize) -> Option<Box<dyn GameObject>> {
        let mut found = false;
        let mut index = 0;
        for (i, object) in self.layers[Layer::index(layer)].iter().enumerate() {
            if object.get_id() == id {
                found = true;
                index = i;
                break;
            }
        }
        if found {
            Some(self.layers[Layer::index(layer)].remove(index))
        } else {
            None
        }
    }
    
    // Put the object in this map cell (and make a creation delta)
    fn put(&mut self, object: Box<dyn GameObject>, delta: &mut DeltaFrame) {
        delta.push(Box::new(CreationDelta::new(&object)));
        self.layers[Layer::index(&object.get_layer())].push(object);
    }
    
    // Put the object in this map cell
    fn put_quiet(&mut self, object: Box<dyn GameObject>) {
        self.layers[Layer::index(&object.get_layer())].push(object);
    }
}

struct WorldMap {
    width: i32,
    height: i32,
    map: Vec<Vec<MapCell>>,
    player: *mut Player,
}

impl WorldMap {
    fn new(width: i32, height: i32, player: *mut Player) -> WorldMap {
        let mut map: Vec<Vec<MapCell>> = Vec::with_capacity(width as usize);
        for i in 0..width as usize {
            map.push(Vec::with_capacity(height as usize));
            for _ in 0..height {
                map[i].push(MapCell::new());
            }
        }
        WorldMap {
            width,
            height,
            map,
            player,
        }
    }
    
    fn get_player_pos(&self) -> (i32, i32) {
        unsafe {
            (*self.player).get_pos()
        }
    }
    
    fn get_player_id(&self) -> usize {
        unsafe {
            (*self.player).get_id()
        }
    }
    
    // NOTE: this (and similar methods later) are predicated on the assumption of "one object per layer per cell"
    fn move_solid(&mut self, (dx, dy): (i32, i32), delta: &mut DeltaFrame) -> bool{
        let layer = &Layer::Solid;
        let mut to_move: HashMap<(i32, i32), usize> = HashMap::new();
        to_move.insert(self.get_player_pos(), self.get_player_id());
        let mut to_check: Vec<(i32, i32)> = Vec::new();
        for (point, _) in to_move.iter() {
            to_check.push(*point);
        }
        // For each iteration: to_move is all points that will be moved if successful
        // to_check is a subset of to_move.
        while !to_check.is_empty() {
            let (x, y) = to_check.pop().unwrap();
            // We've already checked this cell
            if to_move.contains_key(&(x+dx, y+dy)) {
                continue;
            }
            // Something is trying to move out of bounds
            if self.invalid(x+dx, y+dy) {
                return false;
            }
            match self.view(x+dx, y+dy, layer) {
                Some(ref object) => if object.pushable() {
                    to_move.insert((x+dx, y+dy), object.get_id());
                    to_check.push((x+dx, y+dy));
                } else {
                    return false;
                },
                None => {},
            }
        }
        // At this point we are sure the move is legal, so we start moving things
        for ((x, y), id) in to_move.into_iter() {
            let mut object = self.take_id(x, y, layer, id).unwrap();
            object.shift_pos((dx, dy), delta);
            self.put_quiet(object);
        }
        // This is just some random stuff to test creation & deletion deltas (they work!)
        //let (x, y) = self.get_player_pos();
        //if y >= 8 {
        //    if let None = self.view(x, y-7, layer) {
        //        self.put(Box::new(Block::new_block(x, y-7)), delta);
        //    }
        //}
        //self.delete(x, y-1, layer, delta);
        true
    }
    
    // Later, restrict the range based on the camera
    fn draw(&self, canvas: &mut WindowCanvas) {
        for x in 0..self.width {
            for y in 0..self.height {
                self.map[x as usize][y as usize].draw(canvas);
            }
        }
    }
    
    fn invalid(&self, x: i32, y: i32) -> bool {
        x < 0 || x >= self.width || y < 0 || y >= self.height
    }
    
    fn cell(&mut self, x: i32, y: i32) -> Option<&MapCell> {
        if self.invalid(x, y) {
            None
        } else {
            Some(&self.map[x as usize][y as usize])
        }
    }
    
    // Slightly repetitive code lets us avoid unwrapping the inner Option
    // Note that "None" can mean two very different things here!!
    fn view(&mut self, x: i32, y: i32, layer: &Layer) -> Option<&mut Box<dyn GameObject>> {
        if self.invalid(x, y) {
            None
        } else {
            self.map[x as usize][y as usize].view(layer)
        }
    }
    
    fn delete(&mut self, x: i32, y: i32, layer: &Layer, delta: &mut DeltaFrame) -> bool {
        if self.invalid(x, y) {
            false
        } else {
            self.map[x as usize][y as usize].delete(layer, delta)
        }
    }
    
    fn delete_id(&mut self, x: i32, y: i32, layer: &Layer, id: usize, delta: &mut DeltaFrame) -> bool {
        if self.invalid(x, y) {
            false
        } else {
            self.map[x as usize][y as usize].delete_id(layer, id, delta)
        }
    }
    
    fn take(&mut self, x: i32, y: i32, layer: &Layer) -> Option<Box<dyn GameObject>> {
        if self.invalid(x, y) {
            None
        } else {
            self.map[x as usize][y as usize].take(layer)
        }
    }
    
    fn take_id(&mut self, x: i32, y: i32, layer: &Layer, id: usize) -> Option<Box<dyn GameObject>> {
        if self.invalid(x, y) {
            None
        } else {
            self.map[x as usize][y as usize].take_id(layer, id)
        }
    }
    
    // put and put_quiet "should" return Result<(), &str>, but for now they'll just panic
    fn put(&mut self, object: Box<dyn GameObject>, delta: &mut DeltaFrame) {
        let (x, y) = object.get_pos();
        if self.invalid(x, y) {
            panic!("Tried to place an object out of bounds");
        } else {
            self.map[x as usize][y as usize].put(object, delta);
        }
    }
    
    fn put_quiet(&mut self, object: Box<dyn GameObject>) {
        let (x, y) = object.get_pos();
        if self.invalid(x, y) {
            panic!("Tried to place an object out of bounds");
        } else {
            self.map[x as usize][y as usize].put_quiet(object);
        }
    }
}

fn main() {
    let sdl = sdl2::init().unwrap();
    let video_subsystem = sdl.video().unwrap();
    let window = video_subsystem
        .window("Game", SCREEN_WIDTH, SCREEN_HEIGHT)
        .build()
        .unwrap();
        
    let mut canvas = window.into_canvas().build().unwrap();
    
    let mut key_movement = HashMap::new();
    {
        use Keycode::*;
        key_movement.insert(Left, (-1,0));
        key_movement.insert(Right, (1,0));
        key_movement.insert(Down, (0,1));
        key_movement.insert(Up, (0,-1));
    }
    
    // Keep track of the most recently pressed movement key
    let mut buffered_motion_key: Option<Keycode> = None;
    // Was it pressed since the last input was consumed
    let mut buffered_motion_fresh = false;
    
    let mut prev_keys = HashSet::new();
    
    let mut anim_state = AnimationState::Ready;
    
    let mut undo_cooldown = 0;
    
    let mut event_pump = sdl.event_pump().unwrap();
    
    // NOTE: probably not the best way to initialize this...
    let mut player = Box::new(Player::new(3,3));
    let mut world_map = WorldMap::new(10,10, &mut (*player) as *mut Player);
    world_map.put_quiet(player);
    world_map.put_quiet(Box::new(Block::new_wall(5,5)));
    world_map.put_quiet(Box::new(Block::new_block(8,4)));
    
    let mut undo_stack = UndoStack::new(1000);
    
    'mainloop: loop {
        canvas.set_draw_color(Color::RGB(150, 100, 150));
        canvas.clear();
        
        let mut cur_delta_frame = DeltaFrame {
            deltas: vec!(),
        };
        
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit {..}|
                Event::KeyDown {keycode: Some(Keycode::Escape), ..} => {
                    break 'mainloop
                },
                _ => (),
            }
        }
        // Get key presses, releases, and holds
        let keys: HashSet<Keycode> = event_pump.keyboard_state()
            .pressed_scancodes().filter_map(Keycode::from_scancode).collect();
        let new_keys = &keys - &prev_keys;
        
        for key in new_keys.iter() {
            if key_movement.contains_key(key) {
                buffered_motion_key = Some(*key);
                buffered_motion_fresh = true;
            }
        }
        
        match anim_state {
            AnimationState::Ready => {
                // If the buffered key is stale and no longer held, find a new one
                if !buffered_motion_fresh && (
                    buffered_motion_key == None ||
                    !keys.contains(&buffered_motion_key.unwrap())
                ) {
                    buffered_motion_key = None;
                    for (key, _) in key_movement.iter() {
                        if keys.contains(key) {
                            buffered_motion_key = Some(*key);
                        }
                    }
                }
                match buffered_motion_key {
                    Some(key) => {
                        if world_map.move_solid(*key_movement.get(&key).unwrap(), &mut cur_delta_frame) {
                            anim_state = AnimationState::Wait(ANIMATION_LENGTH);
                            // The keypress has been consumed, and is no longer fresh
                            undo_cooldown = 0;
                            buffered_motion_fresh = false;
                        }
                    },
                    None => {},
                }
            },
            AnimationState::Wait(n) => {
                anim_state = if n > 0 {
                    AnimationState::Wait(n-1)
                } else {
                    AnimationState::Ready
                };
            },
        }
        
        if !cur_delta_frame.trivial() {
            undo_stack.push(cur_delta_frame);
        }
        
        if new_keys.contains(&Keycode::Z) {
            undo_stack.pop(&mut world_map);
            undo_cooldown = UNDO_COOLDOWN_MAX;
        } else if keys.contains(&Keycode::Z) {
            if undo_cooldown == 0 {
                undo_stack.pop(&mut world_map);
                undo_cooldown = UNDO_COOLDOWN_MAX;
            }
        }
        
        if undo_cooldown > 0 {
            undo_cooldown -= 1;
        }
        
        world_map.draw(&mut canvas);
        
        prev_keys = keys;
        
        canvas.present();
        ::std::thread::sleep(Duration::new(0, 1_000_000_000u32 / 60));
    }
}