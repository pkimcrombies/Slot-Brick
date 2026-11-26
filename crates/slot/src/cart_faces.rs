use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use slot_gfx::{Compositor, TexId};
use slot_store::Cart;
use slot_ui::{cart_face_with, CartFace};

use crate::app::{App, FaceWant};
use crate::label_cache::label_art;

const WORKERS: usize = 2;

type Key = (u64, usize);

#[derive(Default)]
struct Queue {
    jobs: VecDeque<(Key, Cart)>,
    closed: bool,
}

type Shared = Arc<(Mutex<Queue>, Condvar)>;

pub struct CartFaces {
    queue: Shared,
    built: Receiver<(Key, CartFace, Option<CartFace>)>,
    pending: HashSet<Key>,
    asked: Vec<FaceWant>,
    free: Vec<TexId>,
}

impl CartFaces {
    pub fn spawn(root: PathBuf) -> Self {
        let queue: Shared = Arc::default();
        let (outbox, built) = mpsc::channel();
        for n in 0..WORKERS {
            let (queue, outbox, root) = (queue.clone(), outbox.clone(), root.clone());
            let spawned = thread::Builder::new()
                .name(format!("slot-carts-{n}"))
                .spawn(move || work(&queue, &outbox, &root));
            if let Err(e) = spawned {
                eprintln!("slot: cart faces: worker thread failed to start: {e}");
            }
        }
        CartFaces {
            queue,
            built,
            pending: HashSet::new(),
            asked: Vec::new(),
            free: Vec::new(),
        }
    }

    pub fn sync(&mut self, app: &mut App, compositor: &mut Compositor) {
        self.free.extend(app.shed_faces());
        while let Ok((key, face, box_art)) = self.built.try_recv() {
            self.place(app, compositor, key, face, box_art);
        }
        let wants = app.face_wants();
        if wants != self.asked {
            self.requeue(app, &wants);
            self.asked = wants;
        }
        let mut waiting: Vec<Key> = self
            .asked
            .iter()
            .filter(|w| w.on_screen)
            .map(|w| (w.revision, w.index))
            .collect();
        while !waiting.is_empty() {
            let Ok((key, face, box_art)) = self.built.recv() else {
                return;
            };
            waiting.retain(|k| *k != key);
            self.place(app, compositor, key, face, box_art);
        }
    }

    fn requeue(&mut self, app: &App, wants: &[FaceWant]) {
        let (lock, wake) = &*self.queue;
        let mut queue = lock.lock().unwrap_or_else(|e| e.into_inner());
        for (key, _) in queue.jobs.drain(..) {
            self.pending.remove(&key);
        }
        for w in wants {
            let key = (w.revision, w.index);
            if self.pending.contains(&key) {
                continue;
            }
            if let Some(cart) = app.face_cart(w.index) {
                queue.jobs.push_back((key, cart.clone()));
                self.pending.insert(key);
            }
        }
        wake.notify_all();
    }

    fn place(&mut self, app: &mut App, compositor: &mut Compositor, key: Key, face: CartFace, box_art: Option<CartFace>) {
        self.pending.remove(&key);
        self.asked.retain(|w| (w.revision, w.index) != key);
        if key.0 != app.face_revision() {
            return;
        }
        let tex = match self.free.pop() {
            Some(tex) => {
                compositor.update_texture(tex, face.w, face.h, &face.rgba);
                tex
            }
            None => compositor.create_texture(face.w, face.h, &face.rgba),
        };
        self.free.extend(app.set_cart_face(key.1, tex));
        let box_tex = box_art.map(|art| {
            let tex = match self.free.pop() {
                Some(tex) => { compositor.update_texture(tex, art.w, art.h, &art.rgba); tex }
                None => compositor.create_texture(art.w, art.h, &art.rgba),
            };
            (tex, art.w, art.h)
        });
        self.free.extend(app.set_box_face(key.1, box_tex));
    }
}

impl Drop for CartFaces {
    fn drop(&mut self) {
        let (lock, wake) = &*self.queue;
        lock.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
        wake.notify_all();
    }
}

fn work(queue: &Shared, outbox: &Sender<(Key, CartFace, Option<CartFace>)>, root: &Path) {
    let (lock, wake) = &**queue;
    loop {
        let (key, mut cart) = {
            let mut q = lock.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if q.closed {
                    return;
                }
                if let Some(job) = q.jobs.pop_front() {
                    break job;
                }
                q = wake.wait(q).unwrap_or_else(|e| e.into_inner());
            }
        };
        let face = shelf_face(root, &mut cart);
        let box_art = box_face(root, &cart);
        if outbox.send((key, face, box_art)).is_err() {
            return;
        }
    }
}

/// A shelf cart's face. The library scan leaves ROM headers unread (1.6.0), and a GBA
/// cart's plastic comes from its game code, so the header is read here, off the UI thread.
pub fn shelf_face(root: &Path, cart: &mut Cart) -> CartFace {
    cart.read_header();
    cart_face_with(cart, label_art(root, cart))
}

/// The PNG is independent of cartridge labels and optional. Decode it in the
/// existing shelf worker, not during insertion.
fn box_face(root: &Path, cart: &Cart) -> Option<CartFace> {
    if !matches!(cart.platform, slot_store::Platform::Gb | slot_store::Platform::Gbc | slot_store::Platform::Gba) {
        return None;
    }
    let path = root.join("BoxArt").join(cart.platform.dir_name()).join(format!("{}.png", cart.stem));
    let bytes = match std::fs::File::open(&path) {
        Ok(file) => {
            use std::io::Read;
            let mut bytes = Vec::new();
            if let Err(e) = file.take((8 << 20) + 1).read_to_end(&mut bytes) {
                eprintln!("slot: box art: {}: {e}", path.display());
                return None;
            }
            bytes
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => { eprintln!("slot: box art: {}: {e}", path.display()); return None; }
    };
    if bytes.len() <= (8 << 20) && crate::cart_art::png_size(&bytes)
        .is_some_and(|(w,h)| u64::from(w) * u64::from(h) <= 8 << 20) {
        if let Some((rgba,w,h)) = crate::cart_art::label_picture(&bytes, 576) {
            return Some(CartFace { rgba, w, h });
        }
    }
    eprintln!("slot: box art: invalid or oversized PNG: {}", path.display());
    None
}
