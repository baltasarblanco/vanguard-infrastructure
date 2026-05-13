//! Contrato de memoria compartida entre AEGIS (productor) y CELER (consumidor).
//!
//! Este crate existe para que `StrokeEvent` y los tipos del ring buffer NO puedan
//! divergir entre los dos procesos. Cualquier modificación al layout debe ir
//! acompañada de una actualización al test `abi_contract_truth_serum`, que rompe
//! en compile-time si los offsets cambian sin aviso.

#![allow(dead_code)]

use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Capacidad del ring. DEBE ser potencia de 2 (usamos máscara para indexar).
pub const RING_CAPACITY: usize = 131_072;
const INDEX_MASK: usize = RING_CAPACITY - 1;

/// Action tag for [`StrokeEvent::action`]. Encodes the lifecycle of a single
/// stroke: pen down (start of a new stroke), move (continuation), up (end).
///
/// These constants are the **single source of truth** for the action protocol
/// across the entire pipeline: the browser frontend, AEGIS (producer), and
/// CELER (consumer) all use these exact values. The frontend mirrors them as
/// JavaScript `const`s in `celer/src/index.html` - keep both in sync.
///
/// Values are intentionally small (`u8`) and dense (0/1/2) so the producer
/// can validate range without a lookup table.
pub const ACTION_DOWN: u8 = 0;
pub const ACTION_MOVE: u8 = 1;
pub const ACTION_UP:   u8 = 2;

/// Evento escrito por AEGIS y leído por CELER.
///
/// Alineado a 64 bytes (una línea de caché L1). El compilador agrega padding
/// invisible después de `parent_span_id` para llegar al tamaño total. El layout
/// está fijado por `tests::abi_contract_truth_serum`.
///
/// W3C SpanContext propagado en dos campos, ambos `[u8; N]` para wire format
/// endianness-agnóstico y match directo con la API de
/// `opentelemetry::trace::TraceId` / `opentelemetry::trace::SpanId`:
///   - `trace_id`: 16 bytes. Identifica el trace completo (mismo en todos los
///     spans del árbol). Sentinel "no trace" = `[0u8; 16]` (TraceId::INVALID).
///   - `parent_span_id`: 8 bytes. Identifica el span padre dentro de ese trace.
///     CELER lo usa para reconstruir un `SpanContext` y abrir su span como
///     hijo del span de AEGIS. Sentinel "no parent" = `[0u8; 8]` (SpanId::INVALID).
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy)]
pub struct StrokeEvent {
    pub session_id: u32,
    pub timestamp: u64,
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
    pub action: u8,
    pub trace_id: [u8; 16],
    pub parent_span_id: [u8; 8],
}

/// Acolchado de línea de caché para prevenir false sharing entre `head` y `tail`.
#[repr(align(64))]
pub struct CachePadded<T>(pub T);

/// Header + storage del ring buffer compartido.
///
/// Protocolo SPSC:
///   AEGIS: escribe `events[head & MASK]`, luego `head.store(head+1, Release)`.
///   CELER: observa `head > tail` con `Acquire`, lee `events[tail & MASK]`,
///          luego `tail.store(tail+1, Release)`.
#[repr(C, align(64))]
pub struct SharedRing {
    pub head: CachePadded<AtomicUsize>,
    pub tail: CachePadded<AtomicUsize>,
    pub events: [MaybeUninit<StrokeEvent>; RING_CAPACITY],
}

/// Handle de productor. Lo tiene AEGIS.
///
/// # Safety
/// El llamador garantiza:
/// - `ptr` apunta a un `SharedRing` válido mapeado vía mmap (alineado y vivo).
/// - Solo un `AegisProducer` existe por `SharedRing` (invariante SPSC).
pub struct AegisProducer {
    ring: *mut SharedRing,
    cached_tail: usize,
}

unsafe impl Send for AegisProducer {}

impl AegisProducer {
    /// # Safety
    /// Ver docs del tipo.
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

impl Clone for AegisProducer {
    fn clone(&self) -> Self {
        Self { ring: self.ring, cached_tail: self.cached_tail }
    }
}

/// Handle de consumidor. Lo tiene CELER.
///
/// # Safety
/// Mismas invariantes que `AegisProducer`, exactamente un consumidor por ring.
pub struct CelerConsumer {
    ring: *mut SharedRing,
    cached_head: usize,
}

unsafe impl Send for CelerConsumer {}

impl CelerConsumer {
    /// # Safety
    /// Ver docs del tipo.
    pub unsafe fn new(ptr: *mut SharedRing) -> Self {
        Self { ring: ptr, cached_head: 0 }
    }

    pub fn pop(&mut self) -> Option<StrokeEvent> {
        unsafe {
            let ring = &mut *self.ring;
            let current_tail = ring.tail.0.load(Ordering::Relaxed);

            // Vacío iff head == tail. Usamos wrapping_sub por simetría con `push`
            // y para ser explícitos sobre wraparound (no relevante en 64-bit
            // en la práctica, pero correcto y match con la convención del productor).
            if self.cached_head.wrapping_sub(current_tail) == 0 {
                self.cached_head = ring.head.0.load(Ordering::Acquire);
                if self.cached_head.wrapping_sub(current_tail) == 0 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::alloc::{alloc_zeroed, dealloc, Layout};
    use std::mem::{align_of, offset_of, size_of};

    /// Si este test rompe en compile-time o runtime, alguien tocó el layout
    /// sin actualizar el contrato. PARÁ Y PENSÁ antes de "arreglarlo" cambiando
    /// los números — probablemente el bug está en el cambio, no en el test.
    #[test]
    fn abi_contract_truth_serum() {
        assert_eq!(size_of::<StrokeEvent>(), 64, "StrokeEvent debe ser exactamente 1 cache line");
        assert_eq!(align_of::<StrokeEvent>(), 64);
        assert_eq!(offset_of!(StrokeEvent, session_id), 0);
        assert_eq!(offset_of!(StrokeEvent, timestamp), 8);
        assert_eq!(offset_of!(StrokeEvent, x), 16);
        assert_eq!(offset_of!(StrokeEvent, y), 20);
        assert_eq!(offset_of!(StrokeEvent, pressure), 24);
        assert_eq!(offset_of!(StrokeEvent, action), 28);
        assert_eq!(offset_of!(StrokeEvent, trace_id), 29);
        assert_eq!(offset_of!(StrokeEvent, parent_span_id), 45);
    }

    #[test]
    fn shared_ring_head_tail_separated() {
        let head_off = offset_of!(SharedRing, head);
        let tail_off = offset_of!(SharedRing, tail);
        assert!(tail_off - head_off >= 64, "head y tail deben estar en cache lines distintas");
    }

    /// Roundtrip funcional en el mismo proceso, sin mmap. Verifica que productor
    /// y consumidor se ponen de acuerdo sobre la semántica del ring.
    #[test]
    fn spsc_roundtrip_single_process() {
        let layout = Layout::new::<SharedRing>();
        let ptr = unsafe { alloc_zeroed(layout) as *mut SharedRing };
        assert!(!ptr.is_null());

        let mut producer = unsafe { AegisProducer::new(ptr) };
        let mut consumer = unsafe { CelerConsumer::new(ptr) };

        let trace = [0xab; 16];
        let span = [0xcd; 8];
        let evt = StrokeEvent {
            session_id: 42,
            timestamp: 1_700_000_000_000_000_000,
            x: 100.5,
            y: 200.25,
            pressure: 0.7,
            action: 1,
            trace_id: trace,
            parent_span_id: span,
        };
        producer.push(evt).expect("push should succeed");

        let read = consumer.pop().expect("debería haber un evento");
        assert_eq!(read.session_id, 42);
        assert_eq!(read.x, 100.5);
        assert_eq!(read.y, 200.25);
        assert_eq!(read.pressure, 0.7);
        assert_eq!(read.action, 1);
        assert_eq!(read.trace_id, trace, "trace_id debe sobrevivir el roundtrip byte a byte");
        assert_eq!(read.parent_span_id, span, "parent_span_id debe sobrevivir el roundtrip byte a byte");

        assert!(consumer.pop().is_none(), "ring debería estar vacío");

        unsafe { dealloc(ptr as *mut u8, layout) };
    }

    /// Pin the canonical action protocol. ACTION_DOWN/MOVE/UP must form
    /// a dense contiguous lifecycle 0..3. The frontend, aegis, and celer
    /// all rely on these exact values. Any drift breaks tests here before
    /// it breaks the user-visible system.
    #[test]
    fn action_constants_form_dense_lifecycle() {
        assert_eq!(ACTION_DOWN, 0);
        assert_eq!(ACTION_MOVE, 1);
        assert_eq!(ACTION_UP, 2);
    }
}