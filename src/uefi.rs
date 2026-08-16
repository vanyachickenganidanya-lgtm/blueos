//! Minimal UEFI definitions used by the BlueOS live desktop.
//! Keeping these definitions in-tree avoids a firmware/runtime dependency.

use core::ffi::c_void;

pub type Status = usize;
pub type Handle = *mut c_void;
pub type Event = *mut c_void;
pub type PhysicalAddress = u64;
pub type Lba = u64;

pub const SUCCESS: Status = 0;
pub const NOT_READY: Status = (1usize << (usize::BITS - 1)) | 6;
pub const BUFFER_TOO_SMALL: Status = (1usize << (usize::BITS - 1)) | 5;
pub const BY_PROTOCOL: u32 = 2;
pub const LOADER_DATA: u32 = 2;

#[inline]
pub const fn is_error(status: Status) -> bool {
    status & (1usize << (usize::BITS - 1)) != 0
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

pub const GOP_GUID: Guid = Guid {
    data1: 0x9042_a9de,
    data2: 0x23dc,
    data3: 0x4a38,
    data4: [0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a],
};
pub const SIMPLE_POINTER_GUID: Guid = Guid {
    data1: 0x3187_8c87,
    data2: 0x0b75,
    data3: 0x11d5,
    data4: [0x9a, 0x4f, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
};
pub const SIMPLE_NETWORK_GUID: Guid = Guid {
    data1: 0xa198_32b9,
    data2: 0xac25,
    data3: 0x11d3,
    data4: [0x9a, 0x2d, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
};
pub const BLOCK_IO_GUID: Guid = Guid {
    data1: 0x964e_5b21,
    data2: 0x6459,
    data3: 0x11d2,
    data4: [0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
};
pub const SIMPLE_FILE_SYSTEM_GUID: Guid = Guid {
    data1: 0x964e_5b22,
    data2: 0x6459,
    data3: 0x11d2,
    data4: [0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
};
pub const LOADED_IMAGE_GUID: Guid = Guid {
    data1: 0x5b1b_31a1,
    data2: 0x9562,
    data3: 0x11d2,
    data4: [0x8e, 0x3f, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
};

#[repr(C)]
pub struct TableHeader {
    pub signature: u64,
    pub revision: u32,
    pub header_size: u32,
    pub crc32: u32,
    pub reserved: u32,
}

#[repr(C)]
pub struct InputKey {
    pub scan_code: u16,
    pub unicode_char: u16,
}

#[repr(C)]
pub struct SimpleTextInput {
    pub reset: extern "efiapi" fn(*mut SimpleTextInput, u8) -> Status,
    pub read_key_stroke: extern "efiapi" fn(*mut SimpleTextInput, *mut InputKey) -> Status,
    pub wait_for_key: Event,
}

#[repr(C)]
pub struct SimpleTextOutput {
    pub reset: usize,
    pub output_string: extern "efiapi" fn(*mut SimpleTextOutput, *const u16) -> Status,
    pub test_string: usize,
    pub query_mode: usize,
    pub set_mode: usize,
    pub set_attribute: usize,
    pub clear_screen: usize,
    pub set_cursor_position: usize,
    pub enable_cursor: usize,
    pub mode: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Time {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub pad1: u8,
    pub nanosecond: u32,
    pub time_zone: i16,
    pub daylight: u8,
    pub pad2: u8,
}

#[repr(C)]
pub struct RuntimeServices {
    pub header: TableHeader,
    pub get_time: extern "efiapi" fn(*mut Time, *mut c_void) -> Status,
}

#[repr(C)]
pub struct SystemTable {
    pub header: TableHeader,
    pub firmware_vendor: *const u16,
    pub firmware_revision: u32,
    pub console_in_handle: Handle,
    pub con_in: *mut SimpleTextInput,
    pub console_out_handle: Handle,
    pub con_out: *mut SimpleTextOutput,
    pub standard_error_handle: Handle,
    pub std_err: *mut SimpleTextOutput,
    pub runtime_services: *mut RuntimeServices,
    pub boot_services: *mut BootServices,
    pub number_of_table_entries: usize,
    pub configuration_table: *mut c_void,
}

pub type LocateProtocol = extern "efiapi" fn(*const Guid, *const c_void, *mut *mut c_void) -> Status;
pub type HandleProtocol = extern "efiapi" fn(Handle, *const Guid, *mut *mut c_void) -> Status;
pub type LocateHandleBuffer = extern "efiapi" fn(
    u32,
    *const Guid,
    *const c_void,
    *mut usize,
    *mut *mut Handle,
) -> Status;
pub type AllocatePool = extern "efiapi" fn(u32, usize, *mut *mut c_void) -> Status;
pub type FreePool = extern "efiapi" fn(*mut c_void) -> Status;
pub type Stall = extern "efiapi" fn(usize) -> Status;
pub type SetWatchdogTimer = extern "efiapi" fn(usize, u64, usize, *const u16) -> Status;

#[repr(C)]
pub struct BootServices {
    pub header: TableHeader,
    pub raise_tpl: usize,
    pub restore_tpl: usize,
    pub allocate_pages: usize,
    pub free_pages: usize,
    pub get_memory_map: usize,
    pub allocate_pool: AllocatePool,
    pub free_pool: FreePool,
    pub create_event: usize,
    pub set_timer: usize,
    pub wait_for_event: usize,
    pub signal_event: usize,
    pub close_event: usize,
    pub check_event: usize,
    pub install_protocol_interface: usize,
    pub reinstall_protocol_interface: usize,
    pub uninstall_protocol_interface: usize,
    pub handle_protocol: HandleProtocol,
    pub reserved: usize,
    pub register_protocol_notify: usize,
    pub locate_handle: usize,
    pub locate_device_path: usize,
    pub install_configuration_table: usize,
    pub load_image: usize,
    pub start_image: usize,
    pub exit: usize,
    pub unload_image: usize,
    pub exit_boot_services: usize,
    pub get_next_monotonic_count: usize,
    pub stall: Stall,
    pub set_watchdog_timer: SetWatchdogTimer,
    pub connect_controller: usize,
    pub disconnect_controller: usize,
    pub open_protocol: usize,
    pub close_protocol: usize,
    pub open_protocol_information: usize,
    pub protocols_per_handle: usize,
    pub locate_handle_buffer: LocateHandleBuffer,
    pub locate_protocol: LocateProtocol,
    pub install_multiple_protocol_interfaces: usize,
    pub uninstall_multiple_protocol_interfaces: usize,
    pub calculate_crc32: usize,
    pub copy_mem: usize,
    pub set_mem: usize,
    pub create_event_ex: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GopModeInfo {
    pub version: u32,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    pub pixel_format: u32,
    pub pixel_information: [u32; 4],
    pub pixels_per_scan_line: u32,
}

#[repr(C)]
pub struct GopMode {
    pub max_mode: u32,
    pub mode: u32,
    pub info: *mut GopModeInfo,
    pub size_of_info: usize,
    pub framebuffer_base: PhysicalAddress,
    pub framebuffer_size: usize,
}

pub type QueryMode = extern "efiapi" fn(
    *mut GraphicsOutput,
    u32,
    *mut usize,
    *mut *mut GopModeInfo,
) -> Status;
pub type SetMode = extern "efiapi" fn(*mut GraphicsOutput, u32) -> Status;

#[repr(C)]
pub struct GraphicsOutput {
    pub query_mode: QueryMode,
    pub set_mode: SetMode,
    pub blt: usize,
    pub mode: *mut GopMode,
}

#[repr(C)]
pub struct SimplePointerState {
    pub relative_movement_x: i32,
    pub relative_movement_y: i32,
    pub relative_movement_z: i32,
    pub left_button: u8,
    pub right_button: u8,
}

#[repr(C)]
pub struct SimplePointer {
    pub reset: extern "efiapi" fn(*mut SimplePointer, u8) -> Status,
    pub get_state: extern "efiapi" fn(*mut SimplePointer, *mut SimplePointerState) -> Status,
    pub wait_for_input: Event,
    pub mode: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MacAddress {
    pub address: [u8; 32],
}

#[repr(C)]
pub struct SimpleNetworkMode {
    pub state: u32,
    pub hardware_address_size: u32,
    pub media_header_size: u32,
    pub max_packet_size: u32,
    pub nvram_size: u32,
    pub nvram_access_size: u32,
    pub receive_filter_mask: u32,
    pub receive_filter_setting: u32,
    pub max_mcast_filter_count: u32,
    pub mcast_filter_count: u32,
    pub mcast_filter: [MacAddress; 16],
    pub current_address: MacAddress,
    pub broadcast_address: MacAddress,
    pub permanent_address: MacAddress,
    pub if_type: u8,
    pub mac_address_changeable: u8,
    pub multiple_tx_supported: u8,
    pub media_present_supported: u8,
    pub media_present: u8,
}

pub type SnpGetStatus =
    extern "efiapi" fn(*mut SimpleNetwork, *mut u32, *mut *mut c_void) -> Status;
pub type SnpTransmit = extern "efiapi" fn(
    *mut SimpleNetwork,
    usize,
    usize,
    *mut c_void,
    *mut MacAddress,
    *mut MacAddress,
    *mut u16,
) -> Status;
pub type SnpReceive = extern "efiapi" fn(
    *mut SimpleNetwork,
    *mut usize,
    *mut usize,
    *mut c_void,
    *mut MacAddress,
    *mut MacAddress,
    *mut u16,
) -> Status;

#[repr(C)]
pub struct SimpleNetwork {
    pub revision: u64,
    pub start: extern "efiapi" fn(*mut SimpleNetwork) -> Status,
    pub stop: usize,
    pub initialize: extern "efiapi" fn(*mut SimpleNetwork, usize, usize) -> Status,
    pub reset: usize,
    pub shutdown: usize,
    pub receive_filters: usize,
    pub station_address: usize,
    pub statistics: usize,
    pub mcast_ip_to_mac: usize,
    pub nv_data: usize,
    pub get_status: SnpGetStatus,
    pub transmit: SnpTransmit,
    pub receive: SnpReceive,
    pub wait_for_packet: Event,
    pub mode: *mut SimpleNetworkMode,
}

pub type OpenVolume =
    extern "efiapi" fn(*mut SimpleFileSystem, *mut *mut FileProtocol) -> Status;
pub type CloseFile = extern "efiapi" fn(*mut FileProtocol) -> Status;
pub type ReadFile =
    extern "efiapi" fn(*mut FileProtocol, *mut usize, *mut c_void) -> Status;

#[repr(C)]
pub struct SimpleFileSystem {
    pub revision: u64,
    pub open_volume: OpenVolume,
}

#[repr(C)]
pub struct FileProtocol {
    pub revision: u64,
    pub open: usize,
    pub close: CloseFile,
    pub delete: usize,
    pub read: ReadFile,
    pub write: usize,
    pub get_position: usize,
    pub set_position: usize,
    pub get_info: usize,
    pub set_info: usize,
    pub flush: usize,
    pub open_ex: usize,
    pub read_ex: usize,
    pub write_ex: usize,
    pub flush_ex: usize,
}

#[repr(C)]
pub struct BlockMedia {
    pub media_id: u32,
    pub removable_media: u8,
    pub media_present: u8,
    pub logical_partition: u8,
    pub read_only: u8,
    pub write_caching: u8,
    pub block_size: u32,
    pub io_align: u32,
    pub last_block: Lba,
    pub lowest_aligned_lba: Lba,
    pub logical_blocks_per_physical_block: u32,
    pub optimal_transfer_length_granularity: u32,
}

pub type ResetBlock = extern "efiapi" fn(*mut BlockIo, u8) -> Status;
pub type ReadBlocks = extern "efiapi" fn(*mut BlockIo, u32, Lba, usize, *mut c_void) -> Status;
pub type WriteBlocks = extern "efiapi" fn(*mut BlockIo, u32, Lba, usize, *const c_void) -> Status;
pub type FlushBlocks = extern "efiapi" fn(*mut BlockIo) -> Status;

#[repr(C)]
pub struct BlockIo {
    pub revision: u64,
    pub media: *mut BlockMedia,
    pub reset: ResetBlock,
    pub read_blocks: ReadBlocks,
    pub write_blocks: WriteBlocks,
    pub flush_blocks: FlushBlocks,
}

#[repr(C)]
pub struct LoadedImage {
    pub revision: u32,
    pub parent_handle: Handle,
    pub system_table: *mut SystemTable,
    pub device_handle: Handle,
    pub file_path: *mut c_void,
    pub reserved: *mut c_void,
    pub load_options_size: u32,
    pub load_options: *mut c_void,
    pub image_base: *mut c_void,
    pub image_size: u64,
    pub image_code_type: u32,
    pub image_data_type: u32,
    pub unload: usize,
}
