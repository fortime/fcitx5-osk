#[repr(u32)]
pub enum KeyState {
    NoState = 0x0,
    Shift = 1 << 0,
    CapsLock = 1 << 1,
    Ctrl = 1 << 2,
    Alt = 1 << 3,
    NumLock = 1 << 4,
    Super = 1 << 6,
    Virtual = 1 << 29,
    Repeat = 1 << 31,
}
