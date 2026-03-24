mod ring_buffer;

use nix::sys::memfd::{memfd_create, MFdFlags};
use nix::sys::socket::{sendmsg, ControlMessage, MsgFlags};
use nix::unistd::ftruncate;
use nix::fcntl::{fcntl, FcntlArg, SealFlag};
use memmap2::MmapMut;
use std::ffi::CString;
use std::os::unix::io::{AsRawFd, IntoRawFd};
use std::os::unix::net::UnixStream;

use socket2::{Domain, Protocol, Socket, Type};
use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::thread;
use tokio::sync::broadcast;
use tokio_uring::net::TcpListener;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const BIND_ADDR: &str = "0.0.0.0:8081";      
const CHRONOS_ADDR: &str = "127.0.0.1:8080"; 
const TCP_BACKLOG: i32 = 4096;
const BUFFER_SIZE: usize = 4096;
const POOL_CAPACITY: usize = 1024;
const MAX_HOT_PIPES: usize = 64;

// 💥 RICHARDS VECTOR: Acolchado de Caché
#[repr(align(64))]
struct CachePadded<T>(T);

struct Telemetry {
    active_connections: CachePadded<AtomicUsize>,
    total_bytes: CachePadded<AtomicUsize>,
    total_requests: CachePadded<AtomicUsize>,
}

struct ConnectionGuard {
    tele: Arc<Telemetry>,
}
impl ConnectionGuard {
    fn new(tele: Arc<Telemetry>) -> Self {
        tele.active_connections.0.fetch_add(1, Ordering::Relaxed);
        Self { tele }
    }
}
impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.tele.active_connections.0.fetch_sub(1, Ordering::Relaxed);
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    println!("⚙️ [AEGIS CONTROL PLANE] Iniciando secuencia de arranque...");

    // =======================================================================
    // 🌉 PUENTE FANTASMA IPC & RING BUFFER (AEGIS -> CELER)
    // =======================================================================
    println!("🌉 [AEGIS IPC] Forjando memoria compartida (memfd)...");
    
    let celer_name = CString::new("celer_bridge").unwrap();
    let fd = memfd_create(celer_name.as_c_str(), MFdFlags::MFD_CLOEXEC | MFdFlags::MFD_ALLOW_SEALING)
        .expect("Fallo al crear memfd");

    // Calculamos el tamaño exacto de nuestra estructura RingBuffer (~3.1 MB)
    let ring_size = std::mem::size_of::<ring_buffer::SharedRing>() as i64; 
    ftruncate(&fd, ring_size).expect("Fallo al dimensionar la memoria IPC");

    fcntl(&fd, FcntlArg::F_ADD_SEALS(SealFlag::F_SEAL_SHRINK | SealFlag::F_SEAL_GROW))
        .expect("Fallo al sellar la memoria IPC");

    let mut celer_mmap = unsafe { MmapMut::map_mut(&fd).expect("Fallo al mapear IPC") };

    // Inicializamos el Productor Seguro apuntando a la memoria
    let ring_ptr = celer_mmap.as_mut_ptr() as *mut ring_buffer::SharedRing;
    let producer = unsafe { ring_buffer::AegisProducer::new(ring_ptr) };

    let raw_fd = fd.into_raw_fd(); 

    // HILO 1: El Centinela (Pasa el File Descriptor a CELER)
    thread::spawn(move || {
        println!("⏳ [AEGIS IPC] Hilo centinela buscando a CELER en /tmp/celer_bridge.sock...");
        let mut intentos = 0;
        let stream = loop {
            match UnixStream::connect("/tmp/celer_bridge.sock") {
                Ok(s) => break s,
                Err(_) => {
                    intentos += 1;
                    if intentos % 5 == 0 {
                        println!("⚠️ [AEGIS IPC] CELER no responde. Reintentando conexión...");
                    }
                    std::thread::sleep(Duration::from_secs(2));
                }
            }
        };

        let iov = [std::io::IoSlice::new(b"ping")]; 
        let cmsgs = [ControlMessage::ScmRights(&[raw_fd])];
        
        match sendmsg::<()>(stream.as_raw_fd(), &iov, &cmsgs, MsgFlags::empty(), None) {
            Ok(_) => println!("✅ [AEGIS IPC] ¡Conexión establecida! File Descriptor transferido a CELER."),
            Err(e) => eprintln!("❌ [AEGIS IPC] Fallo crítico al inyectar el FD: {}", e),
        }
    });

    // HILO 2: La Artillería Pesada (Prueba de Estrés Lock-Free L7)
    let producer_ptr = Box::into_raw(Box::new(producer)) as usize; 
    thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(4)); // Le damos 4 seg a Celer para que despierte
        println!("🔥 [AEGIS ARTILLERY] Iniciando fuego de supresión (1 Millón de eventos)...");
        
        let producer = unsafe { &mut *(producer_ptr as *mut ring_buffer::AegisProducer) };
        
        for i in 0..100_000_000 { 
            let event = ring_buffer::NetworkEvent {
            timestamp: i as u64,
            source_ip: 3232235777, // 192.168.1.1
            dest_port: 443,
            protocol: 6,
            flags: 2, // SYN
            payload_len: 0,
        };
    
        // Si la cola está llena, giramos (spin) esperando a que Celer consuma
        while producer.push(event).is_err() {
            std::hint::spin_loop(); // Mechanical Sympathy: Bypass del Kernel
    }
}
// Actualizá también el print final para que no mienta:
println!("🔥 [AEGIS ARTILLERY] Fuego cesado. 100,000,000 eventos inyectados en la RAM.");
// ¡CRUCIAL! No cierres el proceso todavía. 
// Dale 2 segundos a CELER para que termine de vaciar la memoria compartida.
std::thread::sleep(Duration::from_secs(2)); 
println!("💀 [AEGIS CONTROL PLANE] Apagado completo.");
    });
    // =======================================================================

    // A partir de aquí, el código de io_uring sigue intacto
    let core_ids = core_affinity::get_core_ids().expect("Error crítico: No se puede leer la topología");
    let (shutdown_tx, _) = broadcast::channel::<()>(16);
    let addr: SocketAddr = BIND_ADDR.parse().expect("Dirección IP/Puerto inválidos");
    let mut handles = vec![];

    let telemetry = Arc::new(Telemetry {
        active_connections: CachePadded(AtomicUsize::new(0)),
        total_bytes: CachePadded(AtomicUsize::new(0)),
        total_requests: CachePadded(AtomicUsize::new(0)),
    });

    for core_id in core_ids {
        let mut shutdown_rx = shutdown_tx.subscribe();
        let tele_clone = telemetry.clone();

        let handle = thread::spawn(move || {
            core_affinity::set_for_current(core_id);
            
            let mut local_pool = VecDeque::with_capacity(POOL_CAPACITY);
            for _ in 0..POOL_CAPACITY {
                local_pool.push_back(Vec::with_capacity(BUFFER_SIZE));
            }
            let pool = Rc::new(RefCell::new(local_pool));

            let local_conn_pool = VecDeque::with_capacity(MAX_HOT_PIPES);
            let conn_pool = Rc::new(RefCell::new(local_conn_pool));

            let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)).unwrap();
            socket.set_reuse_port(true).unwrap();
            socket.set_reuse_address(true).unwrap();
            socket.set_nonblocking(true).unwrap();
            socket.bind(&addr.into()).unwrap();
            socket.listen(TCP_BACKLOG).unwrap();
            let std_listener: StdTcpListener = socket.into();
            
            tokio_uring::start(async move {
                let listener = TcpListener::from_std(std_listener);
                println!("🚀 [AEGIS-CORE-{}] Motor y Tuberías encendidas.", core_id.id);

                loop {
                    tokio::select! {
                        accept_res = listener.accept() => {
                            if let Ok((stream, _peer_addr)) = accept_res {
                                let pool_ref = pool.clone();
                                let conn_pool_ref = conn_pool.clone();
                                let task_telemetry = tele_clone.clone();
                                
                                tokio_uring::spawn(async move {
                                    let _guard = ConnectionGuard::new(task_telemetry.clone());

                                    let mut buf = pool_ref.borrow_mut().pop_front()
                                        .unwrap_or_else(|| Vec::with_capacity(BUFFER_SIZE));
                                    buf.clear(); 

                                    let chronos_stream = if let Some(hot_pipe) = conn_pool_ref.borrow_mut().pop_front() {
                                        hot_pipe
                                    } else {
                                        let backend_addr: SocketAddr = CHRONOS_ADDR.parse().unwrap();
                                        match tokio_uring::net::TcpStream::connect(backend_addr).await {
                                            Ok(s) => s,
                                            Err(_) => {
                                                pool_ref.borrow_mut().push_back(buf);
                                                return;
                                            }
                                        }
                                    };

                                    let (read_res, buf_read) = stream.read(buf).await;

                                    if let Ok(n) = read_res {
                                        if n > 0 {
                                            task_telemetry.total_bytes.0.fetch_add(n, Ordering::Relaxed);

                                            let (write_res, mut buf_written) = chronos_stream.write_all(buf_read).await;
                                            
                                            if write_res.is_ok() {
                                                buf_written.clear(); 
                                                
                                                let (resp_res, buf_resp) = chronos_stream.read(buf_written).await;

                                                if let Ok(resp_n) = resp_res {
                                                    if resp_n > 0 {
                                                        task_telemetry.total_bytes.0.fetch_add(resp_n, Ordering::Relaxed);

                                                        let (_final_res, mut buf_final) = stream.write_all(buf_resp).await;
                                                        buf_final.clear();
    
                                                        task_telemetry.total_requests.0.fetch_add(1, Ordering::Relaxed);
                                                        
                                                        pool_ref.borrow_mut().push_back(buf_final);
                                                        if conn_pool_ref.borrow().len() < MAX_HOT_PIPES {
                                                            conn_pool_ref.borrow_mut().push_back(chronos_stream);
                                                        }
                                                        return;
                                                    }
                                                }
                                                let mut safe_buf = buf_resp; safe_buf.clear();
                                                pool_ref.borrow_mut().push_back(safe_buf);
                                                return;
                                            }
                                            let mut safe_buf = buf_written; safe_buf.clear();
                                            pool_ref.borrow_mut().push_back(safe_buf);
                                            return;
                                        }
                                    }
                                    let mut safe_buf = buf_read; safe_buf.clear();
                                    pool_ref.borrow_mut().push_back(safe_buf);
                                });
                            }
                        }
                        _ = shutdown_rx.recv() => break,
                    }
                }
            });
        });
        handles.push(handle);
    }

    println!("🛡️ [AEGIS CONTROL PLANE] Todos los sistemas nominales. HUD en terminal Activado.");

    let tele_metrics = telemetry.clone();
    tokio::spawn(async move {
        let listener = match tokio::net::TcpListener::bind("0.0.0.0:8082").await {
            Ok(l) => l,
            Err(_) => {
                println!("⚠️ [TELEMETRÍA] Puerto 8082 ocupado. Modo CLON activado (operando sin satélite).");
                return;
            }
        };
        loop {
            if let Ok((mut stream, _)) = listener.accept().await {
                let tele = tele_metrics.clone();
                tokio::spawn(async move {
                    let mut buf = [0; 512];
                    let _ = stream.read(&mut buf).await;
                    
                    let conns = tele.active_connections.0.load(Ordering::Relaxed);
                    let bytes = tele.total_bytes.0.load(Ordering::Relaxed);
                    let reqs = tele.total_requests.0.load(Ordering::Relaxed);
    
                    let response = format!(
                        "HTTP/1.1 200 OK\r\n\
                        Content-Type: text/plain; version=0.0.4\r\n\
                        Connection: close\r\n\r\n\
                        # HELP aegis_active_connections Numero de conexiones activas\n\
                        # TYPE aegis_active_connections gauge\n\
                        aegis_active_connections {}\n\
                        # HELP aegis_total_bytes Total de bytes\n\
                        # TYPE aegis_total_bytes counter\n\
                        aegis_total_bytes {}\n\
                        # HELP aegis_total_requests Total de peticiones completadas\n\
                        # TYPE aegis_total_requests counter\n\
                        aegis_total_requests {}\n",
                        conns, bytes, reqs
                    );
                    
                    let _ = stream.write_all(response.as_bytes()).await;
                });
            }
        }
    });

    let mut ticker = interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("\n⚠️ [AEGIS CONTROL PLANE] Ctrl+C detectado. Iniciando apagado de la Hidra...");
                break;
            }
            _ = ticker.tick() => {
                let conns = telemetry.active_connections.0.load(Ordering::Relaxed);
                let bytes = telemetry.total_bytes.0.load(Ordering::Relaxed);
                let mb = bytes as f64 / 1_048_576.0;
                
                if conns > 0 || bytes > 0 {
                    println!("📊 [HUD] Conexiones: {} | Tráfico: {:.4} MB", conns, mb);
                }
            }
        }
    }
    
    let _ = shutdown_tx.send(());
    for handle in handles {
        handle.join().unwrap();
    }
    println!("💀 [AEGIS CONTROL PLANE] Apagado completo. Exit Code 0.");
}