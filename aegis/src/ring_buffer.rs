#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::mem::MaybeUninit;

pub const RING_CAPACITY: usize = 131_072;
const INDEX_MASK: usize = RING_CAPACITY - 1;

// 1. EL NUEVO EVENTO GEOMÉTRICO (Fase 2)
// Alineado estrictamente a 64 bytes (L1 Cache Line)
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy)]
pub struct StrokeEvent {
    pub session_id: u32, // ID único del WebSocket/Usuario
    pub timestamp: u64,  // Marca de tiempo en nanosegundos
    pub x: f32,          // Precisión simple para cálculo vectorial rápido
    pub y: f32,
    pub pressure: f32, // 🎯 AGREGÁ ESTA LÍNEA
    pub action: u8,      // 0: Down (Inicio), 1: Move (Trazo), 2: Up (Fin)
    
    // El compilador de Rust rellenará automáticamente el resto 
    // con padding invisible para llegar a los 64 bytes gracias al align(64).
}

// 2. ACOLCHADO DE CACHÉ L1 (Intocable - Prevención de False Sharing)
#[repr(align(64))]
pub struct CachePadded<T>(pub T);

// 3. LA CINTA TRANSPORTADORA
#[repr(C, align(64))]
pub struct SharedRing {
    pub head: CachePadded<AtomicUsize>,
    pub tail: CachePadded<AtomicUsize>,
    // Ahora almacena coordenadas espaciales
    pub events: [MaybeUninit<StrokeEvent>; RING_CAPACITY],
}

// 4. LA INTERFAZ SEGURA PARA AEGIS (El Productor)
pub struct AegisProducer {
    ring: *mut SharedRing,
    cached_tail: usize,
}

impl AegisProducer {
    pub unsafe fn new(ptr: *mut SharedRing) -> Self {
        Self { ring: ptr, cached_tail: 0 }
    }

    pub fn push(&mut self, event: StrokeEvent) -> Result<(), &'static str> {
        unsafe {
            let ring = &mut *self.ring;
            let current_head = ring.head.0.load(Ordering::Relaxed);

            if current_head.wrapping_sub(self.cached_tail) >= RING_CAPACITY {
                self.cached_tail = ring.tail.0.load(Ordering::Acquire);
                if current_head.wrapping_sub(self.cached_tail) >= RING_CAPACITY {
                    return Err("Queue Full");
                }
            }

            let index = current_head & INDEX_MASK;
            ring.events[index].as_mut_ptr().write(event);
            ring.head.0.store(current_head.wrapping_add(1), Ordering::Release);
            Ok(())
        }
    }
}

// 5. LA INTERFAZ SEGURA PARA CELER (El Consumidor)
pub struct CelerConsumer {
    ring: *mut SharedRing,
    cached_head: usize,
}

impl CelerConsumer {
    pub unsafe fn new(ptr: *mut SharedRing) -> Self {
        Self { ring: ptr, cached_head: 0 }
    }

    pub fn pop(&mut self) -> Option<StrokeEvent> {
        unsafe {
            let ring = &mut *self.ring;
            let current_tail = ring.tail.0.load(Ordering::Relaxed);

            if self.cached_head <= current_tail {
                self.cached_head = ring.head.0.load(Ordering::Acquire);
                if self.cached_head <= current_tail {
                    return None;
                }
            }

            let index = current_tail & INDEX_MASK;
            let event = ring.events[index].as_ptr().read();
            ring.tail.0.store(current_tail.wrapping_add(1), Ordering::Release);
            Some(event)
        }
    }
}

impl Clone for AegisProducer {
    fn clone(&self) -> Self {
        Self {
            ring: self.ring,
            cached_tail: self.cached_tail,
        }
    }
}

// También es necesario decirle al compilador que es seguro enviarlo entre hilos
unsafe impl Send for AegisProducer {}