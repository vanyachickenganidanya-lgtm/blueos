use core::{
    ptr::{read_volatile, write_bytes, write_volatile},
    sync::atomic::{fence, Ordering},
};

use crate::{framebuffer::Framebuffer, net::Nic};

const MAGIC: u32 = 0x7472_6976;
const QUEUE_SIZE: usize = 16;

const REG_MAGIC: usize = 0x000;
const REG_VERSION: usize = 0x004;
const REG_DEVICE_ID: usize = 0x008;
const REG_DEVICE_FEATURES: usize = 0x010;
const REG_DEVICE_FEATURES_SEL: usize = 0x014;
const REG_DRIVER_FEATURES: usize = 0x020;
const REG_DRIVER_FEATURES_SEL: usize = 0x024;
const REG_QUEUE_SEL: usize = 0x030;
const REG_QUEUE_NUM_MAX: usize = 0x034;
const REG_QUEUE_NUM: usize = 0x038;
const REG_QUEUE_READY: usize = 0x044;
const REG_QUEUE_NOTIFY: usize = 0x050;
const REG_STATUS: usize = 0x070;
const REG_QUEUE_DESC_LOW: usize = 0x080;
const REG_QUEUE_DESC_HIGH: usize = 0x084;
const REG_QUEUE_DRIVER_LOW: usize = 0x090;
const REG_QUEUE_DRIVER_HIGH: usize = 0x094;
const REG_QUEUE_DEVICE_LOW: usize = 0x0a0;
const REG_QUEUE_DEVICE_HIGH: usize = 0x0a4;

const STATUS_ACKNOWLEDGE: u32 = 1;
const STATUS_DRIVER: u32 = 2;
const STATUS_DRIVER_OK: u32 = 4;
const STATUS_FEATURES_OK: u32 = 8;

#[repr(C)]
#[derive(Clone, Copy)]
struct Descriptor {
    address: u64,
    length: u32,
    flags: u16,
    next: u16,
}

#[repr(C)]
struct Available {
    flags: u16,
    index: u16,
    ring: [u16; QUEUE_SIZE],
    used_event: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UsedElement {
    id: u32,
    length: u32,
}

#[repr(C)]
struct Used {
    flags: u16,
    index: u16,
    ring: [UsedElement; QUEUE_SIZE],
    available_event: u16,
}

#[repr(C, align(4096))]
struct QueueMemory {
    descriptors: [Descriptor; QUEUE_SIZE],
    available: Available,
    used: Used,
}

impl QueueMemory {
    unsafe fn clear(&mut self) {
        write_bytes(self as *mut Self, 0, 1);
    }
}

#[inline]
unsafe fn read(base: usize, offset: usize) -> u32 {
    read_volatile((base + offset) as *const u32)
}

#[inline]
unsafe fn write(base: usize, offset: usize, value: u32) {
    write_volatile((base + offset) as *mut u32, value)
}

fn find_device(device_id: u32) -> Option<usize> {
    for index in 0..8 {
        let base = 0x1000_1000 + index * 0x1000;
        unsafe {
            if read(base, REG_MAGIC) == MAGIC
                && read(base, REG_VERSION) == 2
                && read(base, REG_DEVICE_ID) == device_id
            {
                return Some(base);
            }
        }
    }
    None
}

/// Starts modern virtio negotiation. Returns the offered low feature word.
unsafe fn begin(base: usize) -> Option<u32> {
    write(base, REG_STATUS, 0);
    write(base, REG_STATUS, STATUS_ACKNOWLEDGE);
    write(base, REG_STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);
    write(base, REG_DEVICE_FEATURES_SEL, 1);
    let high = read(base, REG_DEVICE_FEATURES);
    if high & 1 == 0 {
        return None; // VIRTIO_F_VERSION_1
    }
    write(base, REG_DEVICE_FEATURES_SEL, 0);
    Some(read(base, REG_DEVICE_FEATURES))
}

unsafe fn finish_features(base: usize, low_features: u32) -> bool {
    write(base, REG_DRIVER_FEATURES_SEL, 0);
    write(base, REG_DRIVER_FEATURES, low_features);
    write(base, REG_DRIVER_FEATURES_SEL, 1);
    write(base, REG_DRIVER_FEATURES, 1); // VIRTIO_F_VERSION_1 (bit 32)
    let status = STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK;
    write(base, REG_STATUS, status);
    read(base, REG_STATUS) & STATUS_FEATURES_OK != 0
}

unsafe fn setup_queue(base: usize, queue_index: u32, memory: &mut QueueMemory) -> bool {
    write(base, REG_QUEUE_SEL, queue_index);
    let maximum = read(base, REG_QUEUE_NUM_MAX) as usize;
    if maximum < QUEUE_SIZE || read(base, REG_QUEUE_READY) != 0 {
        return false;
    }
    memory.clear();
    write(base, REG_QUEUE_NUM, QUEUE_SIZE as u32);
    let descriptors = core::ptr::addr_of!(memory.descriptors) as u64;
    let available = core::ptr::addr_of!(memory.available) as u64;
    let used = core::ptr::addr_of!(memory.used) as u64;
    write(base, REG_QUEUE_DESC_LOW, descriptors as u32);
    write(base, REG_QUEUE_DESC_HIGH, (descriptors >> 32) as u32);
    write(base, REG_QUEUE_DRIVER_LOW, available as u32);
    write(base, REG_QUEUE_DRIVER_HIGH, (available >> 32) as u32);
    write(base, REG_QUEUE_DEVICE_LOW, used as u32);
    write(base, REG_QUEUE_DEVICE_HIGH, (used >> 32) as u32);
    write(base, REG_QUEUE_READY, 1);
    true
}

/* ---------------------------- virtio-gpu --------------------------- */

const GPU_WIDTH: usize = 800;
const GPU_HEIGHT: usize = 600;
const GPU_RESOURCE: u32 = 1;
const GPU_FORMAT_B8G8R8X8_UNORM: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
struct GpuHeader {
    kind: u32,
    flags: u32,
    fence_id: u64,
    context_id: u32,
    padding: u32,
}

impl GpuHeader {
    const fn command(kind: u32) -> Self {
        Self {
            kind,
            flags: 0,
            fence_id: 0,
            context_id: 0,
            padding: 0,
        }
    }
}

#[repr(C)]
struct ResourceCreate {
    header: GpuHeader,
    resource_id: u32,
    format: u32,
    width: u32,
    height: u32,
}

#[repr(C)]
struct ResourceAttach {
    header: GpuHeader,
    resource_id: u32,
    entries: u32,
    address: u64,
    length: u32,
    padding: u32,
}

#[repr(C, align(8))]
#[derive(Clone, Copy)]
struct Rectangle {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[repr(C)]
struct SetScanout {
    header: GpuHeader,
    rectangle: Rectangle,
    scanout_id: u32,
    resource_id: u32,
}

#[repr(C)]
struct Transfer {
    header: GpuHeader,
    rectangle: Rectangle,
    offset: u64,
    resource_id: u32,
    padding: u32,
}

#[repr(C)]
struct Flush {
    header: GpuHeader,
    rectangle: Rectangle,
    resource_id: u32,
    padding: u32,
}

#[repr(C, align(4096))]
struct GpuFramebuffer([u32; GPU_WIDTH * GPU_HEIGHT]);

static mut GPU_QUEUE: QueueMemory = unsafe { core::mem::zeroed() };
static mut GPU_FRAMEBUFFER: GpuFramebuffer = GpuFramebuffer([0; GPU_WIDTH * GPU_HEIGHT]);

pub struct VirtioGpu {
    base: usize,
    last_used: u16,
}

impl VirtioGpu {
    pub fn probe() -> Option<Self> {
        let base = find_device(16)?;
        unsafe {
            begin(base)?;
            if !finish_features(base, 0)
                || !setup_queue(base, 0, &mut *core::ptr::addr_of_mut!(GPU_QUEUE))
            {
                return None;
            }
            write(
                base,
                REG_STATUS,
                STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK,
            );
        }
        let mut gpu = Self { base, last_used: 0 };
        if gpu.create_resource() {
            Some(gpu)
        } else {
            None
        }
    }

    pub fn framebuffer(&self) -> Framebuffer {
        unsafe {
            Framebuffer::new(
                core::ptr::addr_of_mut!(GPU_FRAMEBUFFER.0) as usize,
                GPU_WIDTH,
                GPU_HEIGHT,
                GPU_WIDTH * 4,
                32,
            )
        }
    }

    fn create_resource(&mut self) -> bool {
        let rectangle = Rectangle {
            x: 0,
            y: 0,
            width: GPU_WIDTH as u32,
            height: GPU_HEIGHT as u32,
        };
        let create = ResourceCreate {
            header: GpuHeader::command(0x0101),
            resource_id: GPU_RESOURCE,
            format: GPU_FORMAT_B8G8R8X8_UNORM,
            width: GPU_WIDTH as u32,
            height: GPU_HEIGHT as u32,
        };
        if !self.command(&create) {
            return false;
        }
        let attach = ResourceAttach {
            header: GpuHeader::command(0x0106),
            resource_id: GPU_RESOURCE,
            entries: 1,
            address: unsafe { core::ptr::addr_of!(GPU_FRAMEBUFFER.0) as u64 },
            length: (GPU_WIDTH * GPU_HEIGHT * 4) as u32,
            padding: 0,
        };
        if !self.command(&attach) {
            return false;
        }
        let scanout = SetScanout {
            header: GpuHeader::command(0x0103),
            rectangle,
            scanout_id: 0,
            resource_id: GPU_RESOURCE,
        };
        self.command(&scanout)
    }

    pub fn flush(&mut self) {
        let rectangle = Rectangle {
            x: 0,
            y: 0,
            width: GPU_WIDTH as u32,
            height: GPU_HEIGHT as u32,
        };
        let transfer = Transfer {
            header: GpuHeader::command(0x0105),
            rectangle,
            offset: 0,
            resource_id: GPU_RESOURCE,
            padding: 0,
        };
        let _ = self.command(&transfer);
        let flush = Flush {
            header: GpuHeader::command(0x0104),
            rectangle,
            resource_id: GPU_RESOURCE,
            padding: 0,
        };
        let _ = self.command(&flush);
    }

    fn command<T>(&mut self, request: &T) -> bool {
        let mut response = GpuHeader::command(0);
        let queue = unsafe { &mut *core::ptr::addr_of_mut!(GPU_QUEUE) };
        queue.descriptors[0] = Descriptor {
            address: request as *const T as u64,
            length: core::mem::size_of::<T>() as u32,
            flags: 1,
            next: 1,
        };
        queue.descriptors[1] = Descriptor {
            address: &mut response as *mut GpuHeader as u64,
            length: core::mem::size_of::<GpuHeader>() as u32,
            flags: 2,
            next: 0,
        };
        let available_index = unsafe { read_volatile(core::ptr::addr_of!(queue.available.index)) };
        queue.available.ring[available_index as usize % QUEUE_SIZE] = 0;
        fence(Ordering::SeqCst);
        unsafe {
            write_volatile(
                core::ptr::addr_of_mut!(queue.available.index),
                available_index.wrapping_add(1),
            );
            write(self.base, REG_QUEUE_NOTIFY, 0);
        }
        for _ in 0..20_000_000 {
            let used = unsafe { read_volatile(core::ptr::addr_of!(queue.used.index)) };
            if used != self.last_used {
                self.last_used = used;
                fence(Ordering::SeqCst);
                return (0x1100..=0x1104).contains(&response.kind);
            }
            core::hint::spin_loop();
        }
        false
    }
}

/* ---------------------------- virtio-net --------------------------- */

const NET_HEADER: usize = 10;
const NET_BUFFER: usize = 2048;
const NET_RX_COUNT: usize = 8;

#[repr(C, align(4096))]
struct NetworkBuffers([[u8; NET_HEADER + NET_BUFFER]; NET_RX_COUNT]);
#[repr(C, align(4096))]
struct TransmitBuffer([u8; NET_HEADER + NET_BUFFER]);

static mut NET_RX_QUEUE: QueueMemory = unsafe { core::mem::zeroed() };
static mut NET_TX_QUEUE: QueueMemory = unsafe { core::mem::zeroed() };
static mut NET_RX_BUFFERS: NetworkBuffers = NetworkBuffers([[0; NET_HEADER + NET_BUFFER]; NET_RX_COUNT]);
static mut NET_TX_BUFFER: TransmitBuffer = TransmitBuffer([0; NET_HEADER + NET_BUFFER]);

pub struct VirtioNet {
    base: usize,
    mac: [u8; 6],
    rx_used: u16,
    tx_used: u16,
    tx_inflight: bool,
}

impl VirtioNet {
    pub fn probe() -> Option<Self> {
        let base = find_device(1)?;
        unsafe {
            let offered = begin(base)?;
            let features = offered & (1 << 5); // VIRTIO_NET_F_MAC only
            if !finish_features(base, features)
                || !setup_queue(base, 0, &mut *core::ptr::addr_of_mut!(NET_RX_QUEUE))
                || !setup_queue(base, 1, &mut *core::ptr::addr_of_mut!(NET_TX_QUEUE))
            {
                return None;
            }
            let queue = &mut *core::ptr::addr_of_mut!(NET_RX_QUEUE);
            for index in 0..NET_RX_COUNT {
                queue.descriptors[index] = Descriptor {
                    address: core::ptr::addr_of_mut!(NET_RX_BUFFERS.0[index]) as u64,
                    length: (NET_HEADER + NET_BUFFER) as u32,
                    flags: 2,
                    next: 0,
                };
                queue.available.ring[index] = index as u16;
            }
            write_volatile(core::ptr::addr_of_mut!(queue.available.index), NET_RX_COUNT as u16);
            write(
                base,
                REG_STATUS,
                STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK,
            );
            fence(Ordering::SeqCst);
            write(base, REG_QUEUE_NOTIFY, 0);

            let mac = if features & (1 << 5) != 0 {
                let config = (base + 0x100) as *const u8;
                [
                    read_volatile(config),
                    read_volatile(config.add(1)),
                    read_volatile(config.add(2)),
                    read_volatile(config.add(3)),
                    read_volatile(config.add(4)),
                    read_volatile(config.add(5)),
                ]
            } else {
                [0x52, 0x54, 0x00, 0x12, 0x34, 0x57]
            };
            Some(Self {
                base,
                mac,
                rx_used: 0,
                tx_used: 0,
                tx_inflight: false,
            })
        }
    }
}

impl Nic for VirtioNet {
    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn receive(&mut self, packet: &mut [u8]) -> Option<usize> {
        let queue = unsafe { &mut *core::ptr::addr_of_mut!(NET_RX_QUEUE) };
        let used_index = unsafe { read_volatile(core::ptr::addr_of!(queue.used.index)) };
        if used_index == self.rx_used {
            return None;
        }
        fence(Ordering::SeqCst);
        let element = unsafe {
            read_volatile(core::ptr::addr_of!(
                queue.used.ring[self.rx_used as usize % QUEUE_SIZE]
            ))
        };
        self.rx_used = self.rx_used.wrapping_add(1);
        let id = element.id as usize;
        if id >= NET_RX_COUNT {
            return None;
        }
        let length = (element.length as usize)
            .saturating_sub(NET_HEADER)
            .min(packet.len())
            .min(NET_BUFFER);
        unsafe {
            let source = core::ptr::addr_of!(NET_RX_BUFFERS.0[id]) as *const u8;
            for (index, output) in packet[..length].iter_mut().enumerate() {
                *output = read_volatile(source.add(NET_HEADER + index));
            }
            let available = read_volatile(core::ptr::addr_of!(queue.available.index));
            queue.available.ring[available as usize % QUEUE_SIZE] = id as u16;
            fence(Ordering::SeqCst);
            write_volatile(
                core::ptr::addr_of_mut!(queue.available.index),
                available.wrapping_add(1),
            );
            write(self.base, REG_QUEUE_NOTIFY, 0);
        }
        Some(length)
    }

    fn transmit(&mut self, packet: &[u8]) -> bool {
        if packet.len() > NET_BUFFER {
            return false;
        }
        let queue = unsafe { &mut *core::ptr::addr_of_mut!(NET_TX_QUEUE) };
        if self.tx_inflight {
            let used = unsafe { read_volatile(core::ptr::addr_of!(queue.used.index)) };
            if used == self.tx_used {
                return false;
            }
            self.tx_used = used;
            self.tx_inflight = false;
        }
        unsafe {
            let buffer = core::ptr::addr_of_mut!(NET_TX_BUFFER.0) as *mut u8;
            for index in 0..NET_HEADER {
                write_volatile(buffer.add(index), 0);
            }
            for (index, byte) in packet.iter().enumerate() {
                write_volatile(buffer.add(NET_HEADER + index), *byte);
            }
            queue.descriptors[0] = Descriptor {
                address: buffer as u64,
                length: (NET_HEADER + packet.len()) as u32,
                flags: 0,
                next: 0,
            };
            let available = read_volatile(core::ptr::addr_of!(queue.available.index));
            queue.available.ring[available as usize % QUEUE_SIZE] = 0;
            fence(Ordering::SeqCst);
            write_volatile(
                core::ptr::addr_of_mut!(queue.available.index),
                available.wrapping_add(1),
            );
            write(self.base, REG_QUEUE_NOTIFY, 1);
        }
        self.tx_inflight = true;
        true
    }
}
