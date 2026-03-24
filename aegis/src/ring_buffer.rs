#![allow(dead_code)] // Silencia las advertencias de código no utilizado

use std::sync::atomic::{AtomicUsize, Ordering};
use std::mem::MaybeUninit;

// La capacidad debe ser potencia de 2 para usar operaciones bit a bit rápidas (AND) en lugar de Módulo (%)
pub const RING_CAPACITY: usize = 131_072; // ~3.1 MB de RAM
const INDEX_MASK: usize = RING_CAPACITY - 1;

// 1. EL EVENTO TÁCTICO
#[repr(C, align(8))]
#[derive(Debug, Clone, Copy)]
pub struct NetworkEvent {
    pub timestamp: u64,
    pub source_ip: u32,
    pub dest_port: u16,
    pub protocol: u8,
    pub flags: u8,
    pub payload_len: u32,
}

// 2. ACOLCHADO DE CACHÉ L1 (Prevención de False Sharing)
// Obliga al compilador a separar variables atómicas en diferentes líneas de caché (64 bytes)
#[repr(align(64))]
pub struct CachePadded<T>(pub T);

// 3. LA CINTA TRANSPORTADORA (El Ring Buffer)
#[repr(C, align(64))]
pub struct SharedRing {
    // Escrito por AEGIS, Leído por CELER (Alineado a 64 bytes)
    pub head: CachePadded<AtomicUsize>,
    
    // Escrito por CELER, Leído por AEGIS (Alineado a 64 bytes)
    pub tail: CachePadded<AtomicUsize>,
    
    // El array de eventos (MaybeUninit evita que Rust inicialice 3MB de RAM en el arranque)
    pub events: [MaybeUninit<NetworkEvent>; RING_CAPACITY],
}

// 4. LA INTERFAZ SEGURA PARA AEGIS (El Productor)
pub struct AegisProducer {
    ring: *mut SharedRing,
    // Caché local del tail para no asfixiar el bus de memoria atómico
    cached_tail: usize,
}

impl AegisProducer {
    pub unsafe fn new(ptr: *mut SharedRing) -> Self {
        Self { ring: ptr, cached_tail: 0 }
    }

    pub fn push(&mut self, event: NetworkEvent) -> Result<(), &'static str> {
        unsafe {
            let ring = &mut *self.ring;
            let current_head = ring.head.0.load(Ordering::Relaxed);

            // Verificamos si hay espacio (Head - Tail < Capacidad)
            if current_head.wrapping_sub(self.cached_tail) >= RING_CAPACITY {
                // Actualizamos la caché del tail leyendo la variable atómica de CELER
                self.cached_tail = ring.tail.0.load(Ordering::Acquire);
                
                // Si aún así no hay espacio, la cola está llena
                if current_head.wrapping_sub(self.cached_tail) >= RING_CAPACITY {
                    return Err("Queue Full");
                }
            }

            // Calculamos el índice rápido con AND bit a bit
            let index = current_head & INDEX_MASK;
            
            // Escribimos el evento crudo en la memoria
            ring.events[index].as_mut_ptr().write(event);
            
            // Avanzamos el Head publicando el cambio (Release) para que CELER lo vea
            ring.head.0.store(current_head.wrapping_add(1), Ordering::Release);
            Ok(())
        }
    }
}

// 5. LA INTERFAZ SEGURA PARA CELER (El Consumidor)
pub struct CelerConsumer {
    ring: *mut SharedRing,
    // Caché local del head para no asfixiar el bus de memoria atómico
    cached_head: usize,
}

impl CelerConsumer {
    pub unsafe fn new(ptr: *mut SharedRing) -> Self {
        Self { ring: ptr, cached_head: 0 }
    }

    pub fn pop(&mut self) -> Option<NetworkEvent> {
        unsafe {
            let ring = &mut *self.ring;
            let current_tail = ring.tail.0.load(Ordering::Relaxed);

            // Verificamos si hay nuevos eventos (Head > Tail)
            if self.cached_head <= current_tail {
                // Actualizamos la caché del head leyendo la variable atómica de AEGIS
                self.cached_head = ring.head.0.load(Ordering::Acquire);
                
                // Si aún así no hay nuevos, la cola está vacía
                if self.cached_head <= current_tail {
                    return None;
                }
            }

            // Calculamos el índice rápido con AND bit a bit
            let index = current_tail & INDEX_MASK;
            
            // Leemos el evento crudo de la memoria
            let event = ring.events[index].as_ptr().read();
            
            // Avanzamos el Tail publicando el cambio (Release) para que AEGIS libere el espacio
            ring.tail.0.store(current_tail.wrapping_add(1), Ordering::Release);
            Some(event)
        }
    }
}