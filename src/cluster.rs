use crate::db::{key_slot, LocalCache};
use crate::network::BatchWriter;
use futures_lite::io::{AsyncReadExt, AsyncWriteExt};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Total number of hash slots in VelocityDB Cluster (matching Redis Cluster specification).
pub const TOTAL_SLOTS: usize = 16384;

/// Size of the slot bitmap in bytes (16384 bits = 2048 bytes).
pub const SLOT_BITMAP_BYTES: usize = TOTAL_SLOTS / 8;

/// Default offset added to the client port to obtain the cluster bus port.
pub const CLUSTER_BUS_PORT_OFFSET: u16 = 10000;

/// Magic 4-byte header identifying VelocityDB / Redis Cluster bus frames ("RCmb").
pub const CLUSTER_BUS_MAGIC: [u8; 4] = *b"RCmb";

/// Gossip message types.
pub const CLUSTERMSG_TYPE_PING: u16 = 0;
pub const CLUSTERMSG_TYPE_PONG: u16 = 1;
pub const CLUSTERMSG_TYPE_MEET: u16 = 2;
pub const CLUSTERMSG_TYPE_FAIL: u16 = 3;
pub const CLUSTERMSG_TYPE_UPDATE: u16 = 4;

/// Cluster node flag masks.
pub const CLUSTER_NODE_MYSELF: u16 = 1 << 0;
pub const CLUSTER_NODE_MASTER: u16 = 1 << 1;
pub const CLUSTER_NODE_SLAVE: u16 = 1 << 2;
pub const CLUSTER_NODE_PFAIL: u16 = 1 << 3;
pub const CLUSTER_NODE_FAIL: u16 = 1 << 4;
pub const CLUSTER_NODE_HANDSHAKE: u16 = 1 << 5;

/// 2048-byte bitmap representing ownership of the 16384 cluster hash slots.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SlotBitmap {
    pub bytes: [u8; SLOT_BITMAP_BYTES],
}

impl std::fmt::Debug for SlotBitmap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SlotBitmap(count: {})", self.count_set())
    }
}

impl Default for SlotBitmap {
    fn default() -> Self {
        Self::new()
    }
}

impl SlotBitmap {
    /// Creates an empty slot bitmap with 0 slots set.
    pub const fn new() -> Self {
        Self {
            bytes: [0u8; SLOT_BITMAP_BYTES],
        }
    }

    /// Creates a full slot bitmap with all 16384 slots assigned.
    pub const fn full() -> Self {
        Self {
            bytes: [0xFFu8; SLOT_BITMAP_BYTES],
        }
    }

    /// Sets ownership of the specified hash slot.
    #[inline]
    pub fn set(&mut self, slot: u16) {
        if (slot as usize) < TOTAL_SLOTS {
            let byte = (slot / 8) as usize;
            let bit = slot % 8;
            self.bytes[byte] |= 1 << bit;
        }
    }

    /// Clears ownership of the specified hash slot.
    #[inline]
    pub fn clear(&mut self, slot: u16) {
        if (slot as usize) < TOTAL_SLOTS {
            let byte = (slot / 8) as usize;
            let bit = slot % 8;
            self.bytes[byte] &= !(1 << bit);
        }
    }

    /// Returns `true` if the node owns the specified hash slot.
    #[inline]
    pub fn is_set(&self, slot: u16) -> bool {
        if (slot as usize) < TOTAL_SLOTS {
            let byte = (slot / 8) as usize;
            let bit = slot % 8;
            (self.bytes[byte] & (1 << bit)) != 0
        } else {
            false
        }
    }

    /// Counts set bits using 64-bit hardware POPCNT instructions.
    #[inline]
    pub fn count_set(&self) -> usize {
        crate::db::bitcount_slice(&self.bytes)
    }

    /// Compresses contiguous set slots into `(start, end)` ranges.
    pub fn to_ranges(&self) -> Vec<(u16, u16)> {
        let mut ranges = Vec::new();
        let mut in_range = false;
        let mut start = 0u16;

        for slot in 0..TOTAL_SLOTS as u16 {
            if self.is_set(slot) {
                if !in_range {
                    in_range = true;
                    start = slot;
                }
            } else if in_range {
                ranges.push((start, slot - 1));
                in_range = false;
            }
        }
        if in_range {
            ranges.push((start, (TOTAL_SLOTS - 1) as u16));
        }
        ranges
    }
}

/// Role of a node in the cluster.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeRole {
    Master,
    Replica,
}

/// Information about a cluster node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClusterNode {
    pub id: String,
    pub ip: String,
    pub port: u16,
    pub bus_port: u16,
    pub role: NodeRole,
    pub master_id: Option<String>,
    pub config_epoch: u64,
    pub slots: SlotBitmap,
    pub flags: u16,
    pub ping_sent: u64,
    pub pong_recv: u64,
    pub link_state: String,
}

impl ClusterNode {
    pub fn new(id: String, ip: String, port: u16, bus_port: u16, role: NodeRole) -> Self {
        let flags = match role {
            NodeRole::Master => CLUSTER_NODE_MASTER,
            NodeRole::Replica => CLUSTER_NODE_SLAVE,
        };
        Self {
            id,
            ip,
            port,
            bus_port,
            role,
            master_id: None,
            config_epoch: 1,
            slots: SlotBitmap::new(),
            flags,
            ping_sent: 0,
            pong_recv: 0,
            link_state: "connected".to_string(),
        }
    }

    pub fn format_flags(&self, is_myself: bool) -> String {
        let mut parts = Vec::new();
        if is_myself {
            parts.push("myself");
        }
        if self.role == NodeRole::Master {
            parts.push("master");
        } else {
            parts.push("slave");
        }
        if (self.flags & CLUSTER_NODE_FAIL) != 0 {
            parts.push("fail");
        } else if (self.flags & CLUSTER_NODE_PFAIL) != 0 {
            parts.push("fail?");
        }
        if (self.flags & CLUSTER_NODE_HANDSHAKE) != 0 {
            parts.push("handshake");
        }
        parts.join(",")
    }
}

/// Redirection decision for a key query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RedirectDecision {
    /// Slot belongs to the local node (or client sent ASKING for importing slot).
    Local,
    /// Slot belongs to another node. Returns `-MOVED <slot> <target_ip>:<target_port>\r\n`.
    Moved { slot: u16, target_addr: String },
    /// Slot is migrating and key does not exist locally. Returns `-ASK <slot> <target_ip>:<target_port>\r\n`.
    Ask { slot: u16, target_addr: String },
    /// Hash slot is not served by any node in the cluster.
    ClusterDown,
}

/// The cluster topology map maintaining 16384 slot mappings and node information.
#[derive(Clone, Debug)]
pub struct ClusterTopology {
    pub my_id: String,
    pub my_ip: String,
    pub my_port: u16,
    pub my_bus_port: u16,
    pub current_epoch: u64,
    pub config_epoch: u64,
    pub nodes: HashMap<String, ClusterNode>,
    /// Direct O(1) map from slot number to owning node ID.
    pub slot_owners: Vec<Option<String>>,
    /// Slots currently being migrated from this node to a target node (`slot -> target_node_id`).
    pub migrating_slots: HashMap<u16, String>,
    /// Slots currently being imported from a source node to this node (`slot -> source_node_id`).
    pub importing_slots: HashMap<u16, String>,
    /// Whether cluster mode is active.
    pub enabled: bool,
}

impl Default for ClusterTopology {
    fn default() -> Self {
        Self::new("127.0.0.1", 6379)
    }
}

impl ClusterTopology {
    /// Creates a new cluster topology for this node.
    pub fn new(ip: &str, port: u16) -> Self {
        let bus_port = port + CLUSTER_BUS_PORT_OFFSET;
        let my_id = generate_node_id(ip, port, 42);
        let mut slot_owners = Vec::with_capacity(TOTAL_SLOTS);
        for _ in 0..TOTAL_SLOTS {
            slot_owners.push(None);
        }

        let myself = ClusterNode::new(my_id.clone(), ip.to_string(), port, bus_port, NodeRole::Master);
        let mut nodes = HashMap::new();
        nodes.insert(my_id.clone(), myself);

        Self {
            my_id,
            my_ip: ip.to_string(),
            my_port: port,
            my_bus_port: bus_port,
            current_epoch: 1,
            config_epoch: 1,
            nodes,
            slot_owners,
            migrating_slots: HashMap::new(),
            importing_slots: HashMap::new(),
            enabled: false,
        }
    }

    /// Assigns slots to this node.
    pub fn add_slots(&mut self, slots: &[u16]) {
        for &slot in slots {
            if (slot as usize) < TOTAL_SLOTS {
                self.slot_owners[slot as usize] = Some(self.my_id.clone());
                if let Some(me) = self.nodes.get_mut(&self.my_id) {
                    me.slots.set(slot);
                }
            }
        }
    }

    /// Removes slots from this node.
    pub fn del_slots(&mut self, slots: &[u16]) {
        for &slot in slots {
            if (slot as usize) < TOTAL_SLOTS {
                self.slot_owners[slot as usize] = None;
                if let Some(me) = self.nodes.get_mut(&self.my_id) {
                    me.slots.clear(slot);
                }
            }
        }
    }

    /// Sets the slot's owner node explicitly.
    pub fn set_slot_node(&mut self, slot: u16, node_id: &str) {
        if (slot as usize) < TOTAL_SLOTS {
            self.migrating_slots.remove(&slot);
            self.importing_slots.remove(&slot);

            if let Some(me) = self.nodes.get_mut(&self.my_id) {
                if node_id == self.my_id {
                    me.slots.set(slot);
                } else {
                    me.slots.clear(slot);
                }
            }

            if let Some(node) = self.nodes.get_mut(node_id) {
                node.slots.set(slot);
            }

            self.slot_owners[slot as usize] = Some(node_id.to_string());
        }
    }

    /// Marks a slot as migrating from this node to `target_node_id`.
    pub fn set_slot_migrating(&mut self, slot: u16, target_node_id: &str) {
        if (slot as usize) < TOTAL_SLOTS {
            self.importing_slots.remove(&slot);
            self.migrating_slots.insert(slot, target_node_id.to_string());
        }
    }

    /// Marks a slot as importing into this node from `source_node_id`.
    pub fn set_slot_importing(&mut self, slot: u16, source_node_id: &str) {
        if (slot as usize) < TOTAL_SLOTS {
            self.migrating_slots.remove(&slot);
            self.importing_slots.insert(slot, source_node_id.to_string());
        }
    }

    /// Clears migrating and importing states for a slot.
    pub fn set_slot_stable(&mut self, slot: u16) {
        self.migrating_slots.remove(&slot);
        self.importing_slots.remove(&slot);
    }

    /// Registers a peer node discovered via `MEET` or gossip.
    pub fn meet_peer(&mut self, ip: &str, port: u16, bus_port: u16) -> String {
        let peer_id = generate_node_id(ip, port, 101);
        if !self.nodes.contains_key(&peer_id) {
            let peer = ClusterNode::new(peer_id.clone(), ip.to_string(), port, bus_port, NodeRole::Master);
            self.nodes.insert(peer_id.clone(), peer);
        }
        peer_id
    }

    /// Returns `true` if this node owns the specified slot.
    #[inline]
    pub fn is_my_slot(&self, slot: u16) -> bool {
        if let Some(owner) = &self.slot_owners[slot as usize] {
            owner == &self.my_id
        } else {
            false
        }
    }

    /// Returns the owner of the specified slot if assigned.
    #[inline]
    pub fn get_slot_owner(&self, slot: u16) -> Option<&ClusterNode> {
        if (slot as usize) < TOTAL_SLOTS {
            let owner_id = self.slot_owners[slot as usize].as_ref()?;
            self.nodes.get(owner_id)
        } else {
            None
        }
    }

    /// Evaluates key routing deterministically with lock-free redirects.
    pub fn check_key_slot(&self, slot: u16, key_exists_locally: bool, asking: bool) -> RedirectDecision {
        if !self.enabled {
            return RedirectDecision::Local;
        }

        if self.is_my_slot(slot) {
            // If the slot is currently migrating to another node:
            if let Some(target_id) = self.migrating_slots.get(&slot) {
                // If key does not exist locally, redirect with -ASK to the migration target
                if !key_exists_locally {
                    if let Some(target) = self.nodes.get(target_id) {
                        return RedirectDecision::Ask {
                            slot,
                            target_addr: format!("{}:{}", target.ip, target.port),
                        };
                    }
                }
            }
            // Key is present locally or slot is normal: process on this node
            RedirectDecision::Local
        } else {
            // Not owned by this node!
            // If client sent ASKING and slot is importing into this node:
            if asking && self.importing_slots.contains_key(&slot) {
                return RedirectDecision::Local;
            }

            // Redirect with -MOVED to the designated slot owner
            if let Some(owner) = self.get_slot_owner(slot) {
                return RedirectDecision::Moved {
                    slot,
                    target_addr: format!("{}:{}", owner.ip, owner.port),
                };
            }

            // Slot is unassigned
            RedirectDecision::ClusterDown
        }
    }

    /// Formats cluster state in standard `CLUSTER NODES` format.
    pub fn format_cluster_nodes(&self) -> String {
        let mut out = String::new();

        // Ensure myself is emitted first
        if let Some(me) = self.nodes.get(&self.my_id) {
            append_node_line(&mut out, me, true, &self.migrating_slots, &self.importing_slots);
        }

        for (id, node) in &self.nodes {
            if id != &self.my_id {
                append_node_line(&mut out, node, false, &self.migrating_slots, &self.importing_slots);
            }
        }

        out
    }

    /// Formats cluster slot allocation into nested RESP array (`CLUSTER SLOTS`).
    pub fn format_cluster_slots_resp(&self) -> Vec<u8> {
        let mut ranges = Vec::new();

        // Group contiguous slots per owner
        let mut in_range = false;
        let mut start_slot = 0u16;
        let mut current_owner: Option<String> = None;

        for slot in 0..TOTAL_SLOTS as u16 {
            let owner = self.slot_owners[slot as usize].clone();
            if in_range {
                if owner == current_owner {
                    continue;
                } else {
                    if let Some(owner_id) = current_owner.take() {
                        ranges.push((start_slot, slot - 1, owner_id));
                    }
                    in_range = false;
                }
            }

            if owner.is_some() {
                in_range = true;
                start_slot = slot;
                current_owner = owner;
            }
        }

        if in_range {
            if let Some(owner_id) = current_owner {
                ranges.push((start_slot, (TOTAL_SLOTS - 1) as u16, owner_id));
            }
        }

        let mut out = Vec::new();
        out.push(b'*');
        out.extend_from_slice(ranges.len().to_string().as_bytes());
        out.extend_from_slice(b"\r\n");

        for (start, end, owner_id) in ranges {
            let owner = self.nodes.get(&owner_id);
            let ip = owner.map(|n| n.ip.as_str()).unwrap_or("127.0.0.1");
            let port = owner.map(|n| n.port).unwrap_or(6379);

            // Array with [start, end, [master_ip, port, node_id]]
            out.extend_from_slice(b"*3\r\n");
            // 1. start
            out.push(b':');
            out.extend_from_slice(start.to_string().as_bytes());
            out.extend_from_slice(b"\r\n");
            // 2. end
            out.push(b':');
            out.extend_from_slice(end.to_string().as_bytes());
            out.extend_from_slice(b"\r\n");
            // 3. master info [ip, port, node_id]
            out.extend_from_slice(b"*3\r\n");
            out.push(b'$');
            out.extend_from_slice(ip.len().to_string().as_bytes());
            out.extend_from_slice(b"\r\n");
            out.extend_from_slice(ip.as_bytes());
            out.extend_from_slice(b"\r\n");

            out.push(b':');
            out.extend_from_slice(port.to_string().as_bytes());
            out.extend_from_slice(b"\r\n");

            out.push(b'$');
            out.extend_from_slice(owner_id.len().to_string().as_bytes());
            out.extend_from_slice(b"\r\n");
            out.extend_from_slice(owner_id.as_bytes());
            out.extend_from_slice(b"\r\n");
        }

        out
    }

    /// Formats cluster statistics in standard `CLUSTER INFO` format.
    pub fn format_cluster_info(&self) -> String {
        let assigned = self.slot_owners.iter().filter(|s| s.is_some()).count();
        let state = if assigned == TOTAL_SLOTS { "ok" } else { "fail" };
        let known_nodes = self.nodes.len();
        let masters = self.nodes.values().filter(|n| n.role == NodeRole::Master).count();

        format!(
            "cluster_state:{state}\r\n\
             cluster_slots_assigned:{assigned}\r\n\
             cluster_slots_ok:{assigned}\r\n\
             cluster_slots_pfail:0\r\n\
             cluster_slots_fail:0\r\n\
             cluster_known_nodes:{known_nodes}\r\n\
             cluster_size:{masters}\r\n\
             cluster_current_epoch:{}\r\n\
             cluster_my_epoch:{}\r\n\
             cluster_stats_messages_ping_sent:0\r\n\
             cluster_stats_messages_pong_received:0\r\n",
            self.current_epoch, self.config_epoch
        )
    }
}

fn append_node_line(
    out: &mut String,
    node: &ClusterNode,
    is_myself: bool,
    migrating: &HashMap<u16, String>,
    importing: &HashMap<u16, String>,
) {
    let master_id = node.master_id.as_deref().unwrap_or("-");
    let flags = node.format_flags(is_myself);
    let mut line = format!(
        "{} {}:{}@{} {} {} {} {} {} {}",
        node.id,
        node.ip,
        node.port,
        node.bus_port,
        flags,
        master_id,
        node.ping_sent,
        node.pong_recv,
        node.config_epoch,
        node.link_state
    );

    // Append slot ranges
    let ranges = node.slots.to_ranges();
    for (start, end) in ranges {
        if start == end {
            line.push_str(&format!(" {start}"));
        } else {
            line.push_str(&format!(" {start}-{end}"));
        }
    }

    // Append migrating / importing slot annotations if myself
    if is_myself {
        for (&slot, target) in migrating {
            line.push_str(&format!(" [{slot}->-{target}]"));
        }
        for (&slot, source) in importing {
            line.push_str(&format!(" [{slot}-<-{source}]"));
        }
    }

    line.push('\n');
    out.push_str(&line);
}

// -----------------------------------------------------------------------------
// Lock-Free ThreadLocal Topology Cache
// -----------------------------------------------------------------------------

thread_local! {
    static CLUSTER_TOPOLOGY: RefCell<Rc<ClusterTopology>> = RefCell::new(Rc::new(ClusterTopology::default()));
}

/// Returns a reference-counted handle to the thread-local cluster topology with ZERO locking.
pub fn get_topology() -> Rc<ClusterTopology> {
    CLUSTER_TOPOLOGY.with(|cell| Rc::clone(&*cell.borrow()))
}

/// Updates the thread-local cluster topology atomically within the local core.
pub fn update_topology(new_topology: ClusterTopology) {
    CLUSTER_TOPOLOGY.with(|cell| {
        *cell.borrow_mut() = Rc::new(new_topology);
    });
}

/// Modifies the thread-local cluster topology with zero cross-thread locking.
pub fn with_topology_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut ClusterTopology) -> R,
{
    CLUSTER_TOPOLOGY.with(|cell| {
        let mut top = (**cell.borrow()).clone();
        let res = f(&mut top);
        *cell.borrow_mut() = Rc::new(top);
        res
    })
}

/// Toggles cluster mode on or off in the thread-local state.
pub fn set_cluster_enabled(enabled: bool) {
    with_topology_mut(|top| {
        top.enabled = enabled;
    });
}

// -----------------------------------------------------------------------------
// Gossip Protocol & Cluster Bus Frame Encoding
// -----------------------------------------------------------------------------

/// Individual node metadata embedded inside gossip ping/pong frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClusterGossipEntry {
    pub node_id: String,
    pub ping_sent: u32,
    pub pong_recv: u32,
    pub ip: String,
    pub port: u16,
    pub bus_port: u16,
    pub flags: u16,
    pub config_epoch: u64,
}

/// Binary gossip message exchanged over the cluster bus port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClusterMsg {
    pub magic: [u8; 4],
    pub totlen: u32,
    pub msg_type: u16,
    pub gossip_count: u16,
    pub sender_id: String,
    pub my_slots: SlotBitmap,
    pub sender_ip: String,
    pub sender_port: u16,
    pub sender_bus_port: u16,
    pub flags: u16,
    pub current_epoch: u64,
    pub config_epoch: u64,
    pub gossip: Vec<ClusterGossipEntry>,
}

impl ClusterMsg {
    /// Serializes the cluster message into a standard binary frame.
    pub fn encode(&self) -> Vec<u8> {
        let entry_size = 40 + 4 + 4 + 46 + 2 + 2 + 2 + 8; // 108 bytes
        let header_size = 4 + 4 + 2 + 2 + 40 + SLOT_BITMAP_BYTES + 46 + 2 + 2 + 2 + 8 + 8; // 2168 bytes
        let totlen = (header_size + self.gossip.len() * entry_size) as u32;

        let mut buf = Vec::with_capacity(totlen as usize);
        buf.extend_from_slice(&self.magic);
        buf.extend_from_slice(&totlen.to_be_bytes());
        buf.extend_from_slice(&self.msg_type.to_be_bytes());
        buf.extend_from_slice(&(self.gossip.len() as u16).to_be_bytes());

        // sender_id: 40 bytes
        let mut id_bytes = [b'0'; 40];
        let copy_len = self.sender_id.as_bytes().len().min(40);
        id_bytes[..copy_len].copy_from_slice(&self.sender_id.as_bytes()[..copy_len]);
        buf.extend_from_slice(&id_bytes);

        // my_slots: 2048 bytes
        buf.extend_from_slice(&self.my_slots.bytes);

        // sender_ip: 46 bytes
        let mut ip_bytes = [0u8; 46];
        let ip_copy_len = self.sender_ip.as_bytes().len().min(46);
        ip_bytes[..ip_copy_len].copy_from_slice(&self.sender_ip.as_bytes()[..ip_copy_len]);
        buf.extend_from_slice(&ip_bytes);

        buf.extend_from_slice(&self.sender_port.to_be_bytes());
        buf.extend_from_slice(&self.sender_bus_port.to_be_bytes());
        buf.extend_from_slice(&self.flags.to_be_bytes());
        buf.extend_from_slice(&self.current_epoch.to_be_bytes());
        buf.extend_from_slice(&self.config_epoch.to_be_bytes());

        // Gossip entries
        for entry in &self.gossip {
            let mut gid_bytes = [b'0'; 40];
            let glen = entry.node_id.as_bytes().len().min(40);
            gid_bytes[..glen].copy_from_slice(&entry.node_id.as_bytes()[..glen]);
            buf.extend_from_slice(&gid_bytes);

            buf.extend_from_slice(&entry.ping_sent.to_be_bytes());
            buf.extend_from_slice(&entry.pong_recv.to_be_bytes());

            let mut gip_bytes = [0u8; 46];
            let giplen = entry.ip.as_bytes().len().min(46);
            gip_bytes[..giplen].copy_from_slice(&entry.ip.as_bytes()[..giplen]);
            buf.extend_from_slice(&gip_bytes);

            buf.extend_from_slice(&entry.port.to_be_bytes());
            buf.extend_from_slice(&entry.bus_port.to_be_bytes());
            buf.extend_from_slice(&entry.flags.to_be_bytes());
            buf.extend_from_slice(&entry.config_epoch.to_be_bytes());
        }

        buf
    }

    /// Deserializes a binary gossip frame from a byte buffer.
    pub fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        let header_size = 4 + 4 + 2 + 2 + 40 + SLOT_BITMAP_BYTES + 46 + 2 + 2 + 2 + 8 + 8;
        if bytes.len() < header_size {
            return Err("Gossip frame too short");
        }

        let magic: [u8; 4] = bytes[0..4].try_into().map_err(|_| "Invalid magic")?;
        if magic != CLUSTER_BUS_MAGIC {
            return Err("Invalid cluster bus magic header");
        }

        let totlen = u32::from_be_bytes(bytes[4..8].try_into().map_err(|_| "Invalid totlen")?);
        if (bytes.len() as u32) < totlen {
            return Err("Incomplete gossip frame");
        }

        let msg_type = u16::from_be_bytes(bytes[8..10].try_into().map_err(|_| "Invalid type")?);
        let gossip_count = u16::from_be_bytes(bytes[10..12].try_into().map_err(|_| "Invalid count")?);

        let sender_id = String::from_utf8_lossy(&bytes[12..52]).to_string();

        let mut slot_bytes = [0u8; SLOT_BITMAP_BYTES];
        slot_bytes.copy_from_slice(&bytes[52..52 + SLOT_BITMAP_BYTES]);
        let my_slots = SlotBitmap { bytes: slot_bytes };

        let pos = 52 + SLOT_BITMAP_BYTES;
        let ip_raw = &bytes[pos..pos + 46];
        let ip_end = ip_raw.iter().position(|&b| b == 0).unwrap_or(46);
        let sender_ip = String::from_utf8_lossy(&ip_raw[..ip_end]).to_string();

        let pos = pos + 46;
        let sender_port = u16::from_be_bytes(bytes[pos..pos + 2].try_into().map_err(|_| "Invalid port")?);
        let sender_bus_port = u16::from_be_bytes(bytes[pos + 2..pos + 4].try_into().map_err(|_| "Invalid bus port")?);
        let flags = u16::from_be_bytes(bytes[pos + 4..pos + 6].try_into().map_err(|_| "Invalid flags")?);
        let current_epoch = u64::from_be_bytes(bytes[pos + 6..pos + 14].try_into().map_err(|_| "Invalid current epoch")?);
        let config_epoch = u64::from_be_bytes(bytes[pos + 14..pos + 22].try_into().map_err(|_| "Invalid config epoch")?);

        let mut offset = header_size;
        let entry_size = 40 + 4 + 4 + 46 + 2 + 2 + 2 + 8;
        let mut gossip = Vec::with_capacity(gossip_count as usize);

        for _ in 0..gossip_count {
            if offset + entry_size > bytes.len() {
                break;
            }
            let gid = String::from_utf8_lossy(&bytes[offset..offset + 40]).to_string();
            let ping_sent = u32::from_be_bytes(bytes[offset + 40..offset + 44].try_into().map_err(|_| "Gossip ping_sent")?);
            let pong_recv = u32::from_be_bytes(bytes[offset + 44..offset + 48].try_into().map_err(|_| "Gossip pong_recv")?);

            let gip_raw = &bytes[offset + 48..offset + 94];
            let gip_end = gip_raw.iter().position(|&b| b == 0).unwrap_or(46);
            let gip = String::from_utf8_lossy(&gip_raw[..gip_end]).to_string();

            let gport = u16::from_be_bytes(bytes[offset + 94..offset + 96].try_into().map_err(|_| "Gossip port")?);
            let gbus_port = u16::from_be_bytes(bytes[offset + 96..offset + 98].try_into().map_err(|_| "Gossip bus_port")?);
            let gflags = u16::from_be_bytes(bytes[offset + 98..offset + 100].try_into().map_err(|_| "Gossip flags")?);
            let gepoch = u64::from_be_bytes(bytes[offset + 100..offset + 108].try_into().map_err(|_| "Gossip epoch")?);

            gossip.push(ClusterGossipEntry {
                node_id: gid,
                ping_sent,
                pong_recv,
                ip: gip,
                port: gport,
                bus_port: gbus_port,
                flags: gflags,
                config_epoch: gepoch,
            });

            offset += entry_size;
        }

        Ok(ClusterMsg {
            magic,
            totlen,
            msg_type,
            gossip_count,
            sender_id,
            my_slots,
            sender_ip,
            sender_port,
            sender_bus_port,
            flags,
            current_epoch,
            config_epoch,
            gossip,
        })
    }
}

/// Processes an incoming gossip message and optionally generates a PONG response.
pub fn process_incoming_gossip(msg: &ClusterMsg) -> Option<ClusterMsg> {
    with_topology_mut(|top| {
        // 1. Process sender information
        if msg.sender_id != top.my_id {
            if !top.nodes.contains_key(&msg.sender_id) {
                let node = ClusterNode {
                    id: msg.sender_id.clone(),
                    ip: msg.sender_ip.clone(),
                    port: msg.sender_port,
                    bus_port: msg.sender_bus_port,
                    role: if (msg.flags & CLUSTER_NODE_SLAVE) != 0 {
                        NodeRole::Replica
                    } else {
                        NodeRole::Master
                    },
                    master_id: None,
                    config_epoch: msg.config_epoch,
                    slots: msg.my_slots,
                    flags: msg.flags,
                    ping_sent: 0,
                    pong_recv: 1,
                    link_state: "connected".to_string(),
                };
                top.nodes.insert(msg.sender_id.clone(), node);
            } else if let Some(existing) = top.nodes.get_mut(&msg.sender_id) {
                existing.ip = msg.sender_ip.clone();
                existing.port = msg.sender_port;
                existing.bus_port = msg.sender_bus_port;
                existing.pong_recv = 1;
                if msg.config_epoch >= existing.config_epoch {
                    existing.config_epoch = msg.config_epoch;
                    existing.slots = msg.my_slots;
                }
            }

            // Update slot ownership if sender has higher or equal config epoch
            for slot in 0..TOTAL_SLOTS as u16 {
                if msg.my_slots.is_set(slot) {
                    let should_update = match &top.slot_owners[slot as usize] {
                        Some(current_owner) => {
                            if current_owner == &top.my_id {
                                false
                            } else if let Some(owner_node) = top.nodes.get(current_owner) {
                                msg.config_epoch > owner_node.config_epoch
                            } else {
                                true
                            }
                        }
                        None => true,
                    };

                    if should_update {
                        top.slot_owners[slot as usize] = Some(msg.sender_id.clone());
                    }
                }
            }

            top.current_epoch = top.current_epoch.max(msg.current_epoch);
        }

        // 2. Discover new nodes through gossip payload
        for entry in &msg.gossip {
            if entry.node_id != top.my_id && !top.nodes.contains_key(&entry.node_id) {
                let discovered = ClusterNode {
                    id: entry.node_id.clone(),
                    ip: entry.ip.clone(),
                    port: entry.port,
                    bus_port: entry.bus_port,
                    role: if (entry.flags & CLUSTER_NODE_SLAVE) != 0 {
                        NodeRole::Replica
                    } else {
                        NodeRole::Master
                    },
                    master_id: None,
                    config_epoch: entry.config_epoch,
                    slots: SlotBitmap::new(),
                    flags: entry.flags,
                    ping_sent: entry.ping_sent as u64,
                    pong_recv: entry.pong_recv as u64,
                    link_state: "connected".to_string(),
                };
                top.nodes.insert(entry.node_id.clone(), discovered);
            }
        }

        // 3. Generate PONG response for PING / MEET messages
        if msg.msg_type == CLUSTERMSG_TYPE_PING || msg.msg_type == CLUSTERMSG_TYPE_MEET {
            let my_slots = top
                .nodes
                .get(&top.my_id)
                .map(|me| me.slots)
                .unwrap_or_else(SlotBitmap::new);

            let mut gossip_entries = Vec::new();
            for (id, node) in &top.nodes {
                if id != &top.my_id && id != &msg.sender_id && gossip_entries.len() < 10 {
                    gossip_entries.push(ClusterGossipEntry {
                        node_id: node.id.clone(),
                        ping_sent: node.ping_sent as u32,
                        pong_recv: node.pong_recv as u32,
                        ip: node.ip.clone(),
                        port: node.port,
                        bus_port: node.bus_port,
                        flags: node.flags,
                        config_epoch: node.config_epoch,
                    });
                }
            }

            Some(ClusterMsg {
                magic: CLUSTER_BUS_MAGIC,
                totlen: 0,
                msg_type: CLUSTERMSG_TYPE_PONG,
                gossip_count: gossip_entries.len() as u16,
                sender_id: top.my_id.clone(),
                my_slots,
                sender_ip: top.my_ip.clone(),
                sender_port: top.my_port,
                sender_bus_port: top.my_bus_port,
                flags: CLUSTER_NODE_MYSELF | CLUSTER_NODE_MASTER,
                current_epoch: top.current_epoch,
                config_epoch: top.config_epoch,
                gossip: gossip_entries,
            })
        } else {
            None
        }
    })
}

/// Asynchronous cluster bus stream handler exchanging gossip packets over io_uring.
pub async fn handle_cluster_bus_stream<S: AsyncReadExt + AsyncWriteExt + Unpin>(
    mut stream: S,
) -> std::io::Result<()> {
    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }

    if let Ok(msg) = ClusterMsg::decode(&buf[..n]) {
        if let Some(pong) = process_incoming_gossip(&msg) {
            let resp_bytes = pong.encode();
            stream.write_all(&resp_bytes).await?;
            stream.flush().await?;
        }
    }

    Ok(())
}

// -----------------------------------------------------------------------------
// RESP CLUSTER Command Execution Engine
// -----------------------------------------------------------------------------

/// Handles all `CLUSTER <subcommand>` queries compliant with the Redis Cluster specification.
pub async fn handle_cluster_command<W: AsyncWriteExt + Unpin>(
    mut args: impl Iterator<Item = &[u8]>,
    cache: &LocalCache,
    batch_writer: &mut BatchWriter<'_, W>,
) -> std::io::Result<()> {
    let subcmd = match args.next() {
        Some(s) => s,
        None => {
            batch_writer
                .write_slice(b"-ERR Unknown or empty CLUSTER subcommand\r\n")
                .await?;
            return Ok(());
        }
    };

    if subcmd.eq_ignore_ascii_case(b"NODES") {
        let top = get_topology();
        let formatted = top.format_cluster_nodes();
        batch_writer.write_bulk_string(formatted.as_bytes()).await?;
    } else if subcmd.eq_ignore_ascii_case(b"SLOTS") {
        let top = get_topology();
        let resp = top.format_cluster_slots_resp();
        batch_writer.write_slice(&resp).await?;
    } else if subcmd.eq_ignore_ascii_case(b"INFO") {
        let top = get_topology();
        let formatted = top.format_cluster_info();
        batch_writer.write_bulk_string(formatted.as_bytes()).await?;
    } else if subcmd.eq_ignore_ascii_case(b"MYID") {
        let top = get_topology();
        batch_writer.write_bulk_string(top.my_id.as_bytes()).await?;
    } else if subcmd.eq_ignore_ascii_case(b"KEYSLOT") {
        if let Some(key) = args.next() {
            let slot = key_slot(key);
            batch_writer.write_integer(slot as i64).await?;
        } else {
            batch_writer
                .write_slice(b"-ERR wrong number of arguments for 'cluster|keyslot'\r\n")
                .await?;
        }
    } else if subcmd.eq_ignore_ascii_case(b"COUNTKEYSINSLOT") {
        if let Some(slot_b) = args.next() {
            if let Ok(slot_num) = std::str::from_utf8(slot_b).unwrap_or("").parse::<u16>() {
                if (slot_num as usize) < TOTAL_SLOTS {
                    let count = cache.count_keys_in_slot(slot_num);
                    batch_writer.write_integer(count as i64).await?;
                } else {
                    batch_writer.write_slice(b"-ERR Invalid slot number\r\n").await?;
                }
            } else {
                batch_writer.write_slice(b"-ERR Invalid slot number\r\n").await?;
            }
        } else {
            batch_writer
                .write_slice(b"-ERR wrong number of arguments for 'cluster|countkeysinslot'\r\n")
                .await?;
        }
    } else if subcmd.eq_ignore_ascii_case(b"ADDSLOTS") {
        let mut slots = Vec::new();
        let mut err = false;
        for arg in args {
            if let Ok(s) = std::str::from_utf8(arg).unwrap_or("").parse::<u16>() {
                if (s as usize) < TOTAL_SLOTS {
                    slots.push(s);
                } else {
                    err = true;
                    break;
                }
            } else {
                err = true;
                break;
            }
        }
        if err || slots.is_empty() {
            batch_writer.write_slice(b"-ERR Slot out of range or invalid\r\n").await?;
        } else {
            with_topology_mut(|top| {
                top.add_slots(&slots);
            });
            batch_writer.write_slice(b"+OK\r\n").await?;
        }
    } else if subcmd.eq_ignore_ascii_case(b"DELSLOTS") {
        let mut slots = Vec::new();
        let mut err = false;
        for arg in args {
            if let Ok(s) = std::str::from_utf8(arg).unwrap_or("").parse::<u16>() {
                if (s as usize) < TOTAL_SLOTS {
                    slots.push(s);
                } else {
                    err = true;
                    break;
                }
            } else {
                err = true;
                break;
            }
        }
        if err || slots.is_empty() {
            batch_writer.write_slice(b"-ERR Slot out of range or invalid\r\n").await?;
        } else {
            with_topology_mut(|top| {
                top.del_slots(&slots);
            });
            batch_writer.write_slice(b"+OK\r\n").await?;
        }
    } else if subcmd.eq_ignore_ascii_case(b"SETSLOT") {
        let slot = match args.next().and_then(|s| std::str::from_utf8(s).ok()?.parse::<u16>().ok()) {
            Some(s) if (s as usize) < TOTAL_SLOTS => s,
            _ => {
                batch_writer.write_slice(b"-ERR Invalid slot number\r\n").await?;
                return Ok(());
            }
        };
        let action = match args.next() {
            Some(a) => a,
            None => {
                batch_writer
                    .write_slice(b"-ERR wrong number of arguments for 'cluster|setslot'\r\n")
                    .await?;
                return Ok(());
            }
        };

        if action.eq_ignore_ascii_case(b"NODE") {
            if let Some(node_id_b) = args.next() {
                let node_id = String::from_utf8_lossy(node_id_b).to_string();
                with_topology_mut(|top| {
                    top.set_slot_node(slot, &node_id);
                });
                batch_writer.write_slice(b"+OK\r\n").await?;
            } else {
                batch_writer
                    .write_slice(b"-ERR wrong number of arguments for 'cluster|setslot'\r\n")
                    .await?;
            }
        } else if action.eq_ignore_ascii_case(b"MIGRATING") {
            if let Some(target_b) = args.next() {
                let target_node = String::from_utf8_lossy(target_b).to_string();
                with_topology_mut(|top| {
                    top.set_slot_migrating(slot, &target_node);
                });
                batch_writer.write_slice(b"+OK\r\n").await?;
            } else {
                batch_writer
                    .write_slice(b"-ERR wrong number of arguments for 'cluster|setslot'\r\n")
                    .await?;
            }
        } else if action.eq_ignore_ascii_case(b"IMPORTING") {
            if let Some(source_b) = args.next() {
                let source_node = String::from_utf8_lossy(source_b).to_string();
                with_topology_mut(|top| {
                    top.set_slot_importing(slot, &source_node);
                });
                batch_writer.write_slice(b"+OK\r\n").await?;
            } else {
                batch_writer
                    .write_slice(b"-ERR wrong number of arguments for 'cluster|setslot'\r\n")
                    .await?;
            }
        } else if action.eq_ignore_ascii_case(b"STABLE") {
            with_topology_mut(|top| {
                top.set_slot_stable(slot);
            });
            batch_writer.write_slice(b"+OK\r\n").await?;
        } else {
            batch_writer
                .write_slice(b"-ERR Invalid CLUSTER SETSLOT subcommand\r\n")
                .await?;
        }
    } else if subcmd.eq_ignore_ascii_case(b"MEET") {
        let ip = match args.next().and_then(|s| std::str::from_utf8(s).ok()) {
            Some(ip) => ip.to_string(),
            None => {
                batch_writer.write_slice(b"-ERR Invalid IP\r\n").await?;
                return Ok(());
            }
        };
        let port = match args.next().and_then(|s| std::str::from_utf8(s).ok()?.parse::<u16>().ok()) {
            Some(p) => p,
            None => {
                batch_writer.write_slice(b"-ERR Invalid port\r\n").await?;
                return Ok(());
            }
        };
        let bus_port = port + CLUSTER_BUS_PORT_OFFSET;
        with_topology_mut(|top| {
            top.meet_peer(&ip, port, bus_port);
        });
        batch_writer.write_slice(b"+OK\r\n").await?;
    } else {
        batch_writer
            .write_slice(b"-ERR Unknown CLUSTER subcommand\r\n")
            .await?;
    }

    Ok(())
}

/// Generates a reproducible 40-character hex node identifier.
pub fn generate_node_id(ip: &str, port: u16, salt: u64) -> String {
    let mut h1: u64 = 0xcbf29ce484222325;
    let mut h2: u64 = 0x100000001b3;
    let mut h3: u32 = 0x811c9dc5;

    for &b in ip.as_bytes() {
        h1 = (h1 ^ (b as u64)).wrapping_mul(0x100000001b3);
        h2 = (h2 ^ (b as u64)).wrapping_mul(0xcbf29ce484222325);
    }
    h1 ^= (port as u64).wrapping_mul(0x517cc1b727220a95);
    h2 ^= salt.wrapping_mul(0x9e3779b97f4a7c15);
    h3 ^= (port as u32) ^ ((salt >> 32) as u32);

    format!("{:016x}{:016x}{:08x}", h1, h2, h3)
}

// -----------------------------------------------------------------------------
// Unit Tests for Cluster Mode
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use futures_lite::io::Cursor;

    #[test]
    fn test_slot_bitmap_operations_and_ranges() {
        let mut bitmap = SlotBitmap::new();
        assert_eq!(bitmap.count_set(), 0);

        // Assign slots 0..=5000
        for s in 0..=5000 {
            bitmap.set(s);
        }
        assert_eq!(bitmap.count_set(), 5001);
        assert!(bitmap.is_set(0));
        assert!(bitmap.is_set(5000));
        assert!(!bitmap.is_set(5001));

        let ranges = bitmap.to_ranges();
        assert_eq!(ranges, vec![(0, 5000)]);

        // Assign discrete slots
        bitmap.set(6000);
        bitmap.set(6001);
        bitmap.set(16383);
        let ranges2 = bitmap.to_ranges();
        assert_eq!(ranges2, vec![(0, 5000), (6000, 6001), (16383, 16383)]);

        // Clear slot
        bitmap.clear(0);
        assert!(!bitmap.is_set(0));
        assert_eq!(bitmap.count_set(), 5003);
    }

    #[test]
    fn test_threadlocal_topology_lock_free_updates() {
        set_cluster_enabled(true);
        with_topology_mut(|top| {
            top.add_slots(&[100, 101, 102]);
        });

        let top = get_topology();
        assert!(top.enabled);
        assert!(top.is_my_slot(100));
        assert!(top.is_my_slot(101));
        assert!(top.is_my_slot(102));
        assert!(!top.is_my_slot(103));

        set_cluster_enabled(false);
    }

    #[test]
    fn test_cluster_key_routing_moved_and_ask_redirects() {
        let mut top = ClusterTopology::new("127.0.0.1", 7000);
        top.enabled = true;

        // Node 1 (Myself) owns slots 0..=5460
        let my_slots: Vec<u16> = (0..=5460).collect();
        top.add_slots(&my_slots);

        // Node 2 owns slots 5461..=10922
        let node2_id = top.meet_peer("127.0.0.1", 7001, 17001);
        for s in 5461..=10922 {
            top.set_slot_node(s, &node2_id);
        }

        // 1. Slot in my range -> Local
        let decision1 = top.check_key_slot(100, false, false);
        assert_eq!(decision1, RedirectDecision::Local);

        // 2. Slot in Node 2 range -> Moved redirect
        let decision2 = top.check_key_slot(6000, false, false);
        assert_eq!(
            decision2,
            RedirectDecision::Moved {
                slot: 6000,
                target_addr: "127.0.0.1:7001".to_string()
            }
        );

        // 3. Slot unassigned -> ClusterDown
        let decision3 = top.check_key_slot(12000, false, false);
        assert_eq!(decision3, RedirectDecision::ClusterDown);

        // 4. Slot migrating from Myself to Node 2:
        top.set_slot_migrating(200, &node2_id);

        // 4a. Key exists locally -> Local
        let decision4a = top.check_key_slot(200, true, false);
        assert_eq!(decision4a, RedirectDecision::Local);

        // 4b. Key absent locally -> Ask redirect
        let decision4b = top.check_key_slot(200, false, false);
        assert_eq!(
            decision4b,
            RedirectDecision::Ask {
                slot: 200,
                target_addr: "127.0.0.1:7001".to_string()
            }
        );

        // 5. Slot importing into Myself from Node 2:
        top.set_slot_node(6500, &node2_id);
        top.set_slot_importing(6500, &node2_id);

        // 5a. Normal request without ASKING -> Moved redirect to Node 2
        let decision5a = top.check_key_slot(6500, false, false);
        assert_eq!(
            decision5a,
            RedirectDecision::Moved {
                slot: 6500,
                target_addr: "127.0.0.1:7001".to_string()
            }
        );

        // 5b. Request preceded by ASKING -> Local execution
        let decision5b = top.check_key_slot(6500, false, true);
        assert_eq!(decision5b, RedirectDecision::Local);
    }

    #[test]
    fn test_gossip_encoding_decoding_and_convergence() {
        let mut msg = ClusterMsg {
            magic: CLUSTER_BUS_MAGIC,
            totlen: 0,
            msg_type: CLUSTERMSG_TYPE_PING,
            gossip_count: 1,
            sender_id: "0123456789abcdef0123456789abcdef01234567".to_string(),
            my_slots: SlotBitmap::new(),
            sender_ip: "192.168.1.10".to_string(),
            sender_port: 7000,
            sender_bus_port: 17000,
            flags: CLUSTER_NODE_MYSELF | CLUSTER_NODE_MASTER,
            current_epoch: 5,
            config_epoch: 3,
            gossip: vec![ClusterGossipEntry {
                node_id: "fedcba9876543210fedcba9876543210fedcba98".to_string(),
                ping_sent: 1000,
                pong_recv: 2000,
                ip: "192.168.1.11".to_string(),
                port: 7001,
                bus_port: 17001,
                flags: CLUSTER_NODE_MASTER,
                config_epoch: 2,
            }],
        };
        msg.my_slots.set(42);

        let encoded = msg.encode();
        let decoded = ClusterMsg::decode(&encoded).expect("Decoding gossip packet failed");

        assert_eq!(decoded.magic, CLUSTER_BUS_MAGIC);
        assert_eq!(decoded.msg_type, CLUSTERMSG_TYPE_PING);
        assert_eq!(decoded.sender_id, msg.sender_id);
        assert_eq!(decoded.sender_ip, msg.sender_ip);
        assert_eq!(decoded.sender_port, msg.sender_port);
        assert_eq!(decoded.sender_bus_port, msg.sender_bus_port);
        assert!(decoded.my_slots.is_set(42));
        assert_eq!(decoded.gossip.len(), 1);
        assert_eq!(decoded.gossip[0].node_id, msg.gossip[0].node_id);
    }

    #[test]
    fn test_handle_cluster_command_nodes_slots_info() {
        futures_lite::future::block_on(async {
            let cache = LocalCache::new();

            // CLUSTER ADDSLOTS 0 1 2
            let mut out = Cursor::new(Vec::new());
            let mut w = BatchWriter::new(&mut out);
            let args = vec![b"ADDSLOTS".as_slice(), b"0".as_slice(), b"1".as_slice(), b"2".as_slice()];
            handle_cluster_command(args.into_iter(), &cache, &mut w).await.unwrap();
            w.flush_all().await.unwrap();
            assert_eq!(out.into_inner(), b"+OK\r\n");

            // CLUSTER KEYSLOT
            let mut out2 = Cursor::new(Vec::new());
            let mut w2 = BatchWriter::new(&mut out2);
            let args2 = vec![b"KEYSLOT".as_slice(), b"{user1}profile".as_slice()];
            handle_cluster_command(args2.into_iter(), &cache, &mut w2).await.unwrap();
            w2.flush_all().await.unwrap();
            let slot = crate::db::key_slot(b"{user1}profile");
            assert_eq!(out2.into_inner(), format!(":{slot}\r\n").as_bytes());

            // CLUSTER NODES
            let mut out3 = Cursor::new(Vec::new());
            let mut w3 = BatchWriter::new(&mut out3);
            let args3 = vec![b"NODES".as_slice()];
            handle_cluster_command(args3.into_iter(), &cache, &mut w3).await.unwrap();
            w3.flush_all().await.unwrap();
            let nodes_output = String::from_utf8_lossy(&out3.into_inner()).into_owned();
            assert!(nodes_output.contains("myself,master"));

            // CLUSTER INFO
            let mut out4 = Cursor::new(Vec::new());
            let mut w4 = BatchWriter::new(&mut out4);
            let args4 = vec![b"INFO".as_slice()];
            handle_cluster_command(args4.into_iter(), &cache, &mut w4).await.unwrap();
            w4.flush_all().await.unwrap();
            let info_output = String::from_utf8_lossy(&out4.into_inner()).into_owned();
            assert!(info_output.contains("cluster_state:"));
        });
    }
}
