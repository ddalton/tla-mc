//! One search over the nodes of a cluster (`-cluster`). Every node loads
//! the same spec and owns the keys that `owner` gives it: it keeps their
//! part of the seen set, their trace records and their part of each BFS
//! level, all as one machine does (see `check`). A successor whose key
//! another node owns is sent there, with its parent's trace index; the
//! owner decides whether it is new, checks the invariants on it and queues
//! it. Only a new one is sent whole: first its key goes, in a batch of
//! keys, and the owner's network thread claims each in its seen set and
//! answers which were new (most successors are states seen already, and a
//! state is ~1 KB where its key is 8 bytes). Levels stay in step: a node that has sent all of a level's
//! successors says so to every peer, and node 0, the coordinator, starts
//! the next level once every node has reported this one's totals.
//!
//! Each ordered pair of nodes has its own TCP connection, so what a node
//! sends a peer arrives in order. Frames: a u32 length, a kind, a payload.

use crate::check::Failure;
use crate::store::{self, FpSet, State, Trace, NO_PARENT};
use std::collections::VecDeque;
use std::io::{BufReader, BufWriter, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

type R<T> = Result<T, String>;

const HELLO: u8 = 1;
const STATES: u8 = 2;
const DONE: u8 = 3;
const SUMMARY: u8 = 4;
const CONTINUE: u8 = 5;
const STOP: u8 = 6;
const ABORT: u8 = 7;
const TRACE_REQ: u8 = 8;
const TRACE_REP: u8 = 9;
const FINISH: u8 = 10;
/// a worker's batch of keys: which are new?
const ASK: u8 = 11;
/// the answer: a bit per key, set if it was new (and is now claimed)
const ANSWER: u8 = 12;

/// Bytes of state frames queued for one peer before a worker waits: the
/// network, not memory, sets the pace.
const OUT_LIMIT: usize = 64 << 20;
/// How long the nodes wait for one another to start.
const START_SECS: u64 = 180;

/// The node of `n` that owns key `fp`: bits of it that neither the seen
/// set's shards (the top 10) nor its tables (the low ones) use, so each
/// node's part still spreads over every shard.
#[inline]
pub fn owner(fp: u64, n: usize) -> usize {
    ((((fp >> 22) as u32) as u64 * n as u64) >> 32) as usize
}

/// What a node reports at the end of a level; node 0 sums them.
#[derive(Clone, Default)]
pub struct Summary {
    pub queued: u64,
    pub on_disk: u64,
    pub generated: u64,
    pub distinct: u64,
}

/// Node 0's decision at the end of a level.
pub struct Decision {
    pub go: bool,
    /// node 0: the cluster's totals, and the failure that stopped it
    pub totals: Summary,
    pub failure: Option<Failure>,
}

struct Outq {
    /// frames, each with whether it counts toward OUT_LIMIT; queued bytes
    q: Mutex<(VecDeque<(Vec<u8>, bool)>, usize)>,
    cv: Condvar,
}

#[derive(Default)]
struct Inbox {
    /// state frames' payloads, from every peer
    frames: VecDeque<Vec<u8>>,
    /// per peer: the last level it has sent all of its states for
    done: Vec<u64>,
    /// everything else, for the main thread: (peer, kind, payload)
    ctl: VecDeque<(usize, u8, Vec<u8>)>,
    dead: Option<String>,
}

pub struct Net {
    pub me: usize,
    pub n: usize,
    nvars: usize,
    out: Vec<Outq>,
    inbox: Mutex<Inbox>,
    frames_cv: Condvar,
    ctl_cv: Condvar,
    /// set when any node fails or one is lost: every node's workers stop
    pub abort: AtomicBool,
    /// this node's workers through with their part of the level
    finished: AtomicUsize,
    /// the search is over: a peer other than node 0 closing its
    /// connections is expected (it may have been released first)
    closing: AtomicBool,
    senders: Mutex<Vec<JoinHandle<()>>>,
    /// this node's part of the seen set, where peers' keys are claimed
    seen: OnceLock<Arc<FpSet>>,
    /// per worker: answers to its batches of keys, (peer, bits)
    answers: Vec<(Mutex<VecDeque<(usize, Vec<u8>)>>, Condvar)>,
}

fn frame(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(5 + payload.len());
    f.extend_from_slice(&(payload.len() as u32 + 1).to_le_bytes());
    f.push(kind);
    f.extend_from_slice(payload);
    f
}

fn read_frame(r: &mut impl Read) -> std::io::Result<(u8, Vec<u8>)> {
    let mut h = [0u8; 5];
    r.read_exact(&mut h)?;
    let len = u32::from_le_bytes(h[..4].try_into().unwrap()) as usize;
    let mut p = vec![0u8; len.saturating_sub(1)];
    r.read_exact(&mut p)?;
    Ok((h[4], p))
}

/// A STATES payload: the encoded states, then their count (u32).
fn split_block(mut p: Vec<u8>) -> (Vec<u8>, u64) {
    let at = p.len().saturating_sub(4);
    let n = p.get(at..).and_then(|b| b.try_into().ok()).map_or(0, u32::from_le_bytes) as u64;
    p.truncate(at);
    (p, n)
}

fn u64_at(b: &[u8], at: usize) -> R<u64> {
    Ok(u64::from_le_bytes(b.get(at..at + 8).ok_or("truncated cluster message")?.try_into().unwrap()))
}

fn hello(me: usize, n: usize, hash: u64, nvars: usize) -> Vec<u8> {
    let mut p = Vec::new();
    for x in [me as u64, n as u64, hash, nvars as u64] {
        p.extend_from_slice(&x.to_le_bytes());
    }
    frame(HELLO, &p)
}

/// The address of every peer of node `me`, the lookups all made at once:
/// a resolver that takes seconds to answer then costs one lookup, not one
/// for each peer.
fn resolve(addrs: &[String], me: usize) -> R<Vec<(usize, SocketAddr)>> {
    std::thread::scope(|s| {
        let lookups: Vec<_> = addrs.iter().enumerate().filter(|&(p, _)| p != me).map(|(p, a)| (p, a, s.spawn(move || a.to_socket_addrs()))).collect();
        lookups
            .into_iter()
            .map(|(p, a, h)| {
                let sa = h.join().map_err(|_| format!("-cluster: {a}: the lookup panicked"))?.map_err(|e| format!("-cluster: {a}: {e}"))?.next().ok_or(format!("-cluster: {a}: no address"))?;
                Ok((p, sa))
            })
            .collect()
    })
}

impl Net {
    /// Join the cluster as node `me` of `addrs` (host:port each, the same
    /// list on every node): listen on our port, look up every peer, connect
    /// to each, and wait until every peer has connected to us. The wait
    /// starts after the lookups, so a slow resolver does not use it up.
    /// `hash`: the sources'; nodes that loaded different specs or cfgs
    /// refuse each other.
    pub fn start(me: usize, addrs: &[String], hash: u64, nvars: usize, workers: usize) -> R<Arc<Net>> {
        let n = addrs.len();
        if !(2..=256).contains(&n) || me >= n {
            return Err(format!("-cluster: node {me} of {n} (2 to 256 nodes)"));
        }
        let port = addrs[me].rsplit_once(':').and_then(|(_, p)| p.parse::<u16>().ok()).ok_or(format!("-cluster: {} has no port", addrs[me]))?;
        let listener = TcpListener::bind(("0.0.0.0", port)).map_err(|e| format!("-cluster: listening on port {port}: {e}"))?;
        let peers = resolve(addrs, me)?;
        let net = Arc::new(Net {
            me,
            n,
            nvars,
            out: (0..n).map(|_| Outq { q: Mutex::new((VecDeque::new(), 0)), cv: Condvar::new() }).collect(),
            inbox: Mutex::new(Inbox { done: vec![0; n], ..Default::default() }),
            frames_cv: Condvar::new(),
            ctl_cv: Condvar::new(),
            abort: AtomicBool::new(false),
            finished: AtomicUsize::new(0),
            closing: AtomicBool::new(false),
            senders: Mutex::new(Vec::new()),
            seen: OnceLock::new(),
            answers: (0..workers.max(1)).map(|_| (Mutex::new(VecDeque::new()), Condvar::new())).collect(),
        });
        let deadline = Instant::now() + Duration::from_secs(START_SECS);
        // accept every peer's connection while connecting to theirs
        let accept = std::thread::spawn(move || -> R<Vec<(usize, TcpStream)>> {
            listener.set_nonblocking(true).map_err(|e| e.to_string())?;
            let mut got: Vec<(usize, TcpStream)> = Vec::new();
            while got.len() < n - 1 {
                match listener.accept() {
                    Ok((s, from)) => {
                        s.set_nonblocking(false).map_err(|e| e.to_string())?;
                        s.set_read_timeout(Some(Duration::from_secs(30))).map_err(|e| e.to_string())?;
                        let (kind, p) = read_frame(&mut &s).map_err(|e| format!("-cluster: hello from {from}: {e}"))?;
                        let (peer, pn, ph, pv) = (u64_at(&p, 0)? as usize, u64_at(&p, 8)? as usize, u64_at(&p, 16)?, u64_at(&p, 24)? as usize);
                        if kind != HELLO || pn != n || peer >= n || peer == me || got.iter().any(|(q, _)| *q == peer) {
                            return Err(format!("-cluster: {from} is not one of this cluster's nodes"));
                        }
                        if ph != hash || pv != nvars {
                            return Err(format!("-cluster: node {peer} ({from}) loaded different sources (spec or cfg)"));
                        }
                        s.set_read_timeout(None).map_err(|e| e.to_string())?;
                        got.push((peer, s));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if Instant::now() > deadline {
                            return Err(format!("-cluster: {} of {} peers connected in {START_SECS}s", got.len(), n - 1));
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(e) => return Err(format!("-cluster: accepting: {e}")),
                }
            }
            Ok(got)
        });
        for (peer, sa) in peers {
            let addr = &addrs[peer];
            let s = loop {
                match TcpStream::connect_timeout(&sa, Duration::from_secs(5)) {
                    Ok(s) => break s,
                    Err(e) if Instant::now() > deadline => return Err(format!("-cluster: connecting to node {peer} ({addr}): {e}")),
                    Err(_) => std::thread::sleep(Duration::from_millis(200)),
                }
            };
            s.set_nodelay(true).map_err(|e| e.to_string())?;
            (&s).write_all(&hello(me, n, hash, nvars)).map_err(|e| format!("-cluster: node {peer} ({addr}): {e}"))?;
            let w = net.clone();
            net.senders.lock().unwrap().push(std::thread::spawn(move || w.send_loop(peer, s)));
        }
        let incoming = accept.join().map_err(|_| "-cluster: the accepting thread panicked")??;
        for (peer, s) in incoming {
            let r = net.clone();
            std::thread::spawn(move || r.recv_loop(peer, s));
        }
        Ok(net)
    }

    #[inline]
    pub fn owner(&self, fp: u64) -> usize {
        owner(fp, self.n)
    }

    fn peers(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.n).filter(move |&p| p != self.me)
    }

    fn lost(&self, peer: usize, why: String) {
        let closing = self.closing.load(Ordering::Relaxed);
        if closing && (peer != 0 || self.me == 0) {
            return;
        }
        let mut g = self.inbox.lock().unwrap();
        if g.dead.is_none() {
            if !closing {
                eprintln!("tla-mc: node {}: {why}", self.me);
            }
            g.dead = Some(why);
        }
        self.abort.store(true, Ordering::Relaxed);
        drop(g);
        self.frames_cv.notify_all();
        self.ctl_cv.notify_all();
        for (_, cv) in &self.answers {
            cv.notify_all();
        }
        for q in &self.out {
            q.cv.notify_all();
        }
    }

    fn send_loop(&self, peer: usize, s: TcpStream) {
        let mut w = BufWriter::with_capacity(1 << 20, &s);
        let q = &self.out[peer];
        loop {
            let (f, more) = {
                let mut g = q.q.lock().unwrap();
                while g.0.is_empty() {
                    g = q.cv.wait(g).unwrap();
                }
                let (f, counted) = g.0.pop_front().unwrap();
                if counted {
                    g.1 -= f.len();
                }
                q.cv.notify_all();
                (f, !g.0.is_empty())
            };
            // an empty frame: the run is over
            if f.is_empty() {
                let _ = w.flush();
                let _ = s.shutdown(Shutdown::Write);
                return;
            }
            if let Err(e) = w.write_all(&f).and_then(|_| if more { Ok(()) } else { w.flush() }) {
                self.lost(peer, format!("lost node {peer}: {e}"));
                return;
            }
        }
    }

    fn recv_loop(&self, peer: usize, s: TcpStream) {
        let mut r = BufReader::with_capacity(1 << 20, s);
        loop {
            let (kind, p) = match read_frame(&mut r) {
                Ok(x) => x,
                Err(e) => {
                    self.lost(peer, format!("lost node {peer}: {e}"));
                    return;
                }
            };
            match kind {
                STATES => {
                    self.inbox.lock().unwrap().frames.push_back(p);
                    self.frames_cv.notify_one();
                }
                ASK => {
                    // claim each key here, now: the asker sends the state
                    // only if it was new
                    let (Some(seen), true) = (self.seen.get(), p.len() >= 4) else {
                        self.lost(peer, format!("node {peer} asked about keys before the search began"));
                        return;
                    };
                    let keys = p.get(4..).unwrap_or_default();
                    let mut a = p[..4].to_vec();
                    a.resize(4 + (keys.len() / 8).div_ceil(8), 0);
                    for (i, k) in keys.chunks_exact(8).enumerate() {
                        match seen.insert(u64::from_le_bytes(k.try_into().unwrap())) {
                            Ok(true) => a[4 + i / 8] |= 1 << (i % 8),
                            Ok(false) => {}
                            Err(e) => {
                                self.lost(peer, format!("the seen set: {e}"));
                                return;
                            }
                        }
                    }
                    // never waits: the asker's network thread may be
                    // waiting to send to this node
                    self.push(peer, frame(ANSWER, &a), false);
                }
                ANSWER => {
                    let w = p.get(..4).map_or(usize::MAX, |b| u32::from_le_bytes(b.try_into().unwrap()) as usize);
                    let Some(q) = self.answers.get(w) else {
                        self.lost(peer, format!("node {peer} answered for no worker of this node"));
                        return;
                    };
                    q.0.lock().unwrap().push_back((peer, p[4..].to_vec()));
                    q.1.notify_one();
                }
                DONE => {
                    let level = u64_at(&p, 0).unwrap_or(u64::MAX);
                    self.inbox.lock().unwrap().done[peer] = level;
                    self.frames_cv.notify_all();
                }
                ABORT => {
                    if !self.abort.swap(true, Ordering::Relaxed) && self.me == 0 {
                        for q in self.peers().filter(|&q| q != peer) {
                            self.push(q, frame(ABORT, &[]), false);
                        }
                    }
                }
                _ => {
                    // node 0 is done with this node: its connections may
                    // close next, before the main thread has read this
                    if peer == 0 && (kind == STOP || kind == FINISH) {
                        self.closing.store(true, Ordering::Relaxed);
                    }
                    self.inbox.lock().unwrap().ctl.push_back((peer, kind, p));
                    self.ctl_cv.notify_all();
                }
            }
        }
    }

    /// Queue a frame for `peer`; a state frame waits while too many bytes
    /// are queued for it already.
    fn push(&self, peer: usize, f: Vec<u8>, counted: bool) {
        let q = &self.out[peer];
        let mut g = q.q.lock().unwrap();
        if counted {
            while g.1 > OUT_LIMIT && !self.abort.load(Ordering::Relaxed) {
                g = q.cv.wait_timeout(g, Duration::from_millis(100)).unwrap().0;
            }
            g.1 += f.len();
        }
        g.0.push_back((f, counted));
        q.cv.notify_all();
    }

    /// New states (trace index, state) for the queue of the node that owns
    /// them, as a block of a BFS level: the owner keeps the bytes as they
    /// come. The count goes last.
    pub fn send_queued(&self, peer: usize, states: &[(u64, State)]) {
        let mut f = vec![0u8; 5];
        store::encode_block(states, &mut f);
        f.extend_from_slice(&(states.len() as u32).to_le_bytes());
        let len = (f.len() - 4) as u32;
        f[..4].copy_from_slice(&len.to_le_bytes());
        f[4] = STATES;
        self.push(peer, f, true);
    }

    /// This node's part of the seen set, before the search begins.
    pub fn set_seen(&self, seen: Arc<FpSet>) {
        let _ = self.seen.set(seen);
    }

    /// The main thread, before a level's workers start.
    pub fn begin_level(&self) {
        self.finished.store(0, Ordering::SeqCst);
    }

    /// A worker is through with its part of `level` and has queued what
    /// it sends; the last of `workers` tells every peer this node is done.
    pub fn worker_done(&self, workers: usize, level: u64) {
        if self.finished.fetch_add(1, Ordering::SeqCst) + 1 == workers {
            for p in self.peers() {
                self.push(p, frame(DONE, &level.to_le_bytes()), false);
            }
        }
    }

    /// A block of states sent for this node's queue, if one has arrived:
    /// (the states, encoded, and how many).
    pub fn try_frame(&self) -> Option<(Vec<u8>, u64)> {
        self.inbox.lock().unwrap().frames.pop_front().map(split_block)
    }

    /// The next block of `level` sent for this node's queue; None once
    /// every peer has sent all of its own (or a node is lost).
    pub fn next_frame(&self, level: u64) -> Option<(Vec<u8>, u64)> {
        let mut g = self.inbox.lock().unwrap();
        loop {
            if let Some(f) = g.frames.pop_front() {
                return Some(split_block(f));
            }
            if g.dead.is_some() || self.peers().all(|p| g.done[p] >= level) {
                return None;
            }
            g = self.frames_cv.wait(g).unwrap();
        }
    }

    /// This node has failed: stop every node.
    pub fn raise_abort(&self) {
        if self.abort.swap(true, Ordering::Relaxed) {
            return;
        }
        if self.me == 0 {
            for p in self.peers() {
                self.push(p, frame(ABORT, &[]), false);
            }
        } else {
            self.push(0, frame(ABORT, &[]), false);
        }
    }

    /// The next control message from `from` (any peer if None) of one of
    /// `kinds`.
    fn wait_ctl(&self, from: Option<usize>, kinds: &[u8]) -> R<(usize, u8, Vec<u8>)> {
        let mut g = self.inbox.lock().unwrap();
        loop {
            if let Some(i) = g.ctl.iter().position(|(p, k, _)| from.is_none_or(|f| f == *p) && kinds.contains(k)) {
                return Ok(g.ctl.remove(i).unwrap());
            }
            if let Some(d) = &g.dead {
                return Err(format!("cluster: {d}"));
            }
            g = self.ctl_cv.wait(g).unwrap();
        }
    }

    /// The end of a level: every node reports to node 0, which decides for
    /// all whether there is another. A level goes on while any node queued
    /// a state for it and none failed.
    pub fn barrier(&self, s: &Summary, failure: Option<Failure>) -> R<Decision> {
        let mut p = Vec::new();
        for x in [s.queued, s.on_disk, s.generated, s.distinct] {
            p.extend_from_slice(&x.to_le_bytes());
        }
        if self.me != 0 {
            if let Some(f) = &failure {
                encode_failure(f, &mut p);
            }
            self.push(0, frame(SUMMARY, &p), false);
            let (_, kind, _) = self.wait_ctl(Some(0), &[CONTINUE, STOP])?;
            return Ok(Decision { go: kind == CONTINUE, totals: s.clone(), failure: None });
        }
        let mut totals = s.clone();
        let mut fails: Vec<(usize, Failure)> = failure.into_iter().map(|f| (0, f)).collect();
        for _ in 1..self.n {
            let (peer, _, p) = self.wait_ctl(None, &[SUMMARY])?;
            totals.queued += u64_at(&p, 0)?;
            totals.on_disk += u64_at(&p, 8)?;
            totals.generated += u64_at(&p, 16)?;
            totals.distinct += u64_at(&p, 24)?;
            if p.len() > 32 {
                fails.push((peer, decode_failure(&p[32..], self.nvars)?));
            }
        }
        fails.sort_by_key(|(peer, _)| *peer);
        let failure = fails.into_iter().next().map(|(_, f)| f);
        let go = failure.is_none() && totals.queued > 0 && !self.abort.load(Ordering::Relaxed);
        for q in self.peers() {
            self.push(q, frame(if go { CONTINUE } else { STOP }, &[]), false);
        }
        Ok(Decision { go, totals, failure })
    }

    /// Node 0, after the search: the trace record at `idx`, wherever it is.
    pub fn trace_get(&self, trace: &Trace, idx: u64) -> R<[u64; 2]> {
        let node = store::node_of(idx);
        if node == self.me {
            return trace.get(idx);
        }
        if node >= self.n {
            return Err("bad trace index".into());
        }
        self.push(node, frame(TRACE_REQ, &idx.to_le_bytes()), false);
        let (_, _, p) = self.wait_ctl(Some(node), &[TRACE_REP])?;
        if p.len() < 16 {
            return Err(format!("node {node}: {}", String::from_utf8_lossy(&p)));
        }
        Ok([u64_at(&p, 0)?, u64_at(&p, 8)?])
    }

    /// Every node but 0, after the search: answer node 0's trace lookups
    /// until it is done.
    pub fn serve(&self, trace: &Trace) -> R<()> {
        loop {
            let (_, kind, p) = self.wait_ctl(Some(0), &[TRACE_REQ, FINISH])?;
            if kind == FINISH {
                self.shutdown();
                return Ok(());
            }
            let rep = match u64_at(&p, 0).and_then(|i| trace.get(i)) {
                Ok([fp, parent]) => [fp.to_le_bytes(), parent.to_le_bytes()].concat(),
                Err(e) => e.into_bytes(),
            };
            self.push(0, frame(TRACE_REP, &rep), false);
        }
    }

    /// Node 0, at the end: release every node, and wait until they have
    /// been told.
    pub fn finish(&self) {
        self.closing.store(true, Ordering::Relaxed);
        for p in self.peers() {
            self.push(p, frame(FINISH, &[]), false);
        }
        self.shutdown();
    }

    fn shutdown(&self) {
        for p in self.peers() {
            self.push(p, Vec::new(), false);
        }
        for h in self.senders.lock().unwrap().drain(..) {
            let _ = h.join();
        }
    }
}

// ---- a worker's successors bound for other nodes ---------------------------

/// A successor whose key another node owns, waiting to be asked about:
/// which variables differ from its parent's (bit i; all set past 128
/// variables), for checking the invariants on it once it is known new.
pub struct Pending {
    pub fp: u64,
    pub pidx: u64,
    pub chg: u128,
    pub st: State,
}

/// One worker's successors whose keys other nodes own, per node: a batch
/// being filled, the batches whose keys were sent to be claimed, and the
/// new ones, logged and checked here, to be sent to their owner's queue.
pub struct Outgoing {
    w: usize,
    batch: usize,
    filling: Vec<Vec<Pending>>,
    asked: Vec<VecDeque<Vec<Pending>>>,
    new: Vec<Vec<(u64, State)>>,
    keys: Vec<u8>,
}

impl Outgoing {
    /// For worker `w`, `batch` states (or keys) to a message.
    pub fn new(net: &Net, w: usize, batch: usize) -> Outgoing {
        let n = net.n;
        Outgoing { w, batch, filling: (0..n).map(|_| Vec::new()).collect(), asked: (0..n).map(|_| VecDeque::new()).collect(), new: vec![Vec::new(); n], keys: Vec::new() }
    }

    /// A successor that node `to` owns.
    pub fn push(&mut self, net: &Net, to: usize, s: Pending) {
        self.filling[to].push(s);
        if self.filling[to].len() >= self.batch {
            self.ask(net, to);
        }
    }

    fn ask(&mut self, net: &Net, to: usize) {
        if self.filling[to].is_empty() {
            return;
        }
        let b = std::mem::take(&mut self.filling[to]);
        self.keys.clear();
        self.keys.extend_from_slice(&(self.w as u32).to_le_bytes());
        b.iter().for_each(|s| self.keys.extend_from_slice(&s.fp.to_le_bytes()));
        net.push(to, frame(ASK, &self.keys), true);
        self.asked[to].push_back(b);
    }

    /// Ask about every successor not asked about yet.
    pub fn ask_all(&mut self, net: &Net) {
        (0..net.n).for_each(|to| self.ask(net, to));
    }

    /// Batches asked about and not answered yet.
    pub fn waiting(&self) -> bool {
        self.asked.iter().any(|q| !q.is_empty())
    }

    /// The successors the answers that have come say are new, each with
    /// its owner; with `wait`, wait a little for an answer if none has come.
    pub fn answers(&mut self, net: &Net, wait: bool, out: &mut Vec<(usize, Pending)>) -> R<()> {
        let (q, cv) = &net.answers[self.w];
        let mut got = Vec::new();
        {
            let mut q = q.lock().unwrap();
            if q.is_empty() && wait && !net.abort.load(Ordering::Relaxed) {
                q = cv.wait_timeout(q, Duration::from_millis(100)).unwrap().0;
            }
            got.extend(q.drain(..));
        }
        for (from, bits) in got {
            let b = self.asked[from].pop_front().ok_or(format!("cluster: node {from} answered a batch that was not asked"))?;
            if bits.len() != b.len().div_ceil(8) {
                return Err(format!("cluster: node {from} answered for {} keys, {} were asked", bits.len() * 8, b.len()));
            }
            out.extend(b.into_iter().enumerate().filter(|(i, _)| bits[i / 8] & (1 << (i % 8)) != 0).map(|(_, s)| (from, s)));
        }
        Ok(())
    }

    /// A new state, at trace index `idx` here, for node `to`'s queue.
    pub fn send(&mut self, net: &Net, to: usize, idx: u64, st: State) {
        self.new[to].push((idx, st));
        if self.new[to].len() >= self.batch {
            net.send_queued(to, &self.new[to]);
            self.new[to].clear();
        }
    }

    /// The end of the worker's part of a level: send what is left.
    pub fn flush(&mut self, net: &Net) {
        for to in 0..net.n {
            if !self.new[to].is_empty() {
                if !net.abort.load(Ordering::Relaxed) {
                    net.send_queued(to, &self.new[to]);
                }
                self.new[to].clear();
            }
        }
    }
}

// ---- failures, as node 0 is told them --------------------------------------

fn put_str(out: &mut Vec<u8>, s: &str) {
    store::put_var(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

fn get_str(b: &[u8], pos: &mut usize) -> R<String> {
    let n = store::get_var(b, pos)? as usize;
    let s = b.get(*pos..*pos + n).ok_or("truncated cluster message")?;
    *pos += n;
    Ok(String::from_utf8_lossy(s).into_owned())
}

fn encode_failure(f: &Failure, out: &mut Vec<u8>) {
    match f {
        Failure::Invariant(name, idx) => {
            out.push(1);
            put_str(out, name);
            store::put_var(out, *idx);
        }
        Failure::Step(name, idx, st) => {
            out.push(2);
            put_str(out, name);
            store::put_var(out, *idx);
            store::encode_block(&[(0, st.clone())], out);
        }
        Failure::Liveness(name) => {
            out.push(3);
            put_str(out, name);
        }
        Failure::Deadlock(idx) => {
            out.push(4);
            store::put_var(out, *idx);
        }
        Failure::Eval(msg, idx) => {
            out.push(5);
            put_str(out, msg);
            store::put_var(out, idx.unwrap_or(NO_PARENT));
        }
        Failure::InitProperty(name, idx) => {
            out.push(6);
            put_str(out, name);
            store::put_var(out, *idx);
        }
    }
}

fn decode_failure(b: &[u8], nvars: usize) -> R<Failure> {
    let mut pos = 1;
    let tag = *b.first().ok_or("truncated cluster message")?;
    Ok(match tag {
        1 => Failure::Invariant(get_str(b, &mut pos)?, store::get_var(b, &mut pos)?),
        2 => {
            let name = get_str(b, &mut pos)?;
            let idx = store::get_var(b, &mut pos)?;
            let mut st = Vec::new();
            store::decode_states(&b[pos..], 1, nvars, &mut st)?;
            Failure::Step(name, idx, st.pop().ok_or("truncated cluster message")?.1)
        }
        3 => Failure::Liveness(get_str(b, &mut pos)?),
        4 => Failure::Deadlock(store::get_var(b, &mut pos)?),
        5 => {
            let msg = get_str(b, &mut pos)?;
            let idx = store::get_var(b, &mut pos)?;
            Failure::Eval(msg, (idx != NO_PARENT).then_some(idx))
        }
        6 => Failure::InitProperty(get_str(b, &mut pos)?, store::get_var(b, &mut pos)?),
        t => return Err(format!("bad failure tag {t} in a cluster message")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_spreads_keys_and_leaves_shards_alone() {
        // keys from a mixing hash: every node gets about its share, and
        // each node's keys still fall in every one of the seen set's shards
        let n = 6;
        let mut count = vec![0usize; n];
        let mut shards = vec![std::collections::HashSet::new(); n];
        let mut x = 0x9E37_79B9_7F4A_7C15_u64;
        for _ in 0..600_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let o = owner(x, n);
            count[o] += 1;
            shards[o].insert(x >> 54);
        }
        for o in 0..n {
            assert!((90_000..110_000).contains(&count[o]), "node {o}: {}", count[o]);
            assert_eq!(shards[o].len(), 1024);
        }
    }

    #[test]
    fn resolve_gives_every_peer_and_names_a_bad_address() {
        let addrs = ["127.0.0.1:7001", "127.0.0.1:7002", "[::1]:7003"].map(String::from);
        let got = resolve(&addrs, 1).unwrap();
        assert_eq!(got.iter().map(|&(p, sa)| (p, sa.port())).collect::<Vec<_>>(), [(0, 7001), (2, 7003)]);
        let bad = ["127.0.0.1:7001", "127.0.0.1"].map(String::from);
        let e = resolve(&bad, 0).unwrap_err();
        assert!(e.starts_with("-cluster: 127.0.0.1: "), "{e}");
    }

    #[test]
    fn failures_round_trip() {
        use crate::value::Value;
        let st: State = vec![Value::Int(3), Value::Seq(vec![Value::Bool(true), Value::Int(-2)].into())].into_boxed_slice();
        for f in [
            Failure::Invariant("Inv".into(), 7 | (3 << store::NODE_SHIFT)),
            Failure::Step("Prop".into(), 9, st.clone()),
            Failure::Deadlock(11),
            Failure::Eval("boom".into(), None),
            Failure::Eval("boom".into(), Some(5)),
            Failure::InitProperty("P".into(), 0),
        ] {
            let mut b = Vec::new();
            encode_failure(&f, &mut b);
            let g = decode_failure(&b, 2).unwrap();
            let (mut x, mut y) = (Vec::new(), Vec::new());
            encode_failure(&g, &mut y);
            encode_failure(&f, &mut x);
            assert_eq!(x, y);
        }
    }
}
