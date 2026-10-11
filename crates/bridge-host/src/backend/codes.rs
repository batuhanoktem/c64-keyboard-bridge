//! Physical platform key codes. These tables never read host keyboard input.
use bridge_core::NativeKey;

#[cfg(any(target_os = "macos", test))]
pub(super) fn mac_keycode(key: NativeKey) -> u16 {
    use NativeKey::*;
    match key {
        A => 0,
        S => 1,
        D => 2,
        F => 3,
        H => 4,
        G => 5,
        Z => 6,
        X => 7,
        C => 8,
        V => 9,
        IntlBackslash => 10,
        B => 11,
        Q => 12,
        W => 13,
        E => 14,
        R => 15,
        Y => 16,
        T => 17,
        Num1 => 18,
        Num2 => 19,
        Num3 => 20,
        Num4 => 21,
        Num6 => 22,
        Num5 => 23,
        Equal => 24,
        Num9 => 25,
        Num7 => 26,
        Minus => 27,
        Num8 => 28,
        Num0 => 29,
        RightBracket => 30,
        O => 31,
        U => 32,
        LeftBracket => 33,
        I => 34,
        P => 35,
        Enter => 36,
        L => 37,
        J => 38,
        Quote => 39,
        K => 40,
        Semicolon => 41,
        Backslash => 42,
        Comma => 43,
        Slash => 44,
        N => 45,
        M => 46,
        Period => 47,
        Tab => 48,
        Space => 49,
        Grave => 50,
        Backspace => 51,
        Escape => 53,
        Meta => 55,
        LeftShift => 56,
        Alt => 58,
        Control => 59,
        RightShift => 60,
        F5 => 96,
        F6 => 97,
        F7 => 98,
        F3 => 99,
        F8 => 100,
        F9 => 101,
        F11 => 103,
        F10 => 109,
        F12 => 111,
        Home => 115,
        PageUp => 116,
        Delete => 117,
        F4 => 118,
        End => 119,
        F2 => 120,
        PageDown => 121,
        F1 => 122,
        ArrowLeft => 123,
        ArrowRight => 124,
        ArrowDown => 125,
        ArrowUp => 126,
    }
}

#[cfg(any(target_os = "windows", target_os = "linux", test))]
pub(super) fn pc_scancode(key: NativeKey) -> (u16, bool) {
    use NativeKey::*;
    let ordinary = match key {
        Escape => 0x01,
        Num1 => 0x02,
        Num2 => 0x03,
        Num3 => 0x04,
        Num4 => 0x05,
        Num5 => 0x06,
        Num6 => 0x07,
        Num7 => 0x08,
        Num8 => 0x09,
        Num9 => 0x0a,
        Num0 => 0x0b,
        Minus => 0x0c,
        Equal => 0x0d,
        Backspace => 0x0e,
        Tab => 0x0f,
        Q => 0x10,
        W => 0x11,
        E => 0x12,
        R => 0x13,
        T => 0x14,
        Y => 0x15,
        U => 0x16,
        I => 0x17,
        O => 0x18,
        P => 0x19,
        LeftBracket => 0x1a,
        RightBracket => 0x1b,
        Enter => 0x1c,
        Control => 0x1d,
        A => 0x1e,
        S => 0x1f,
        D => 0x20,
        F => 0x21,
        G => 0x22,
        H => 0x23,
        J => 0x24,
        K => 0x25,
        L => 0x26,
        Semicolon => 0x27,
        Quote => 0x28,
        Grave => 0x29,
        LeftShift => 0x2a,
        Backslash => 0x2b,
        Z => 0x2c,
        X => 0x2d,
        C => 0x2e,
        V => 0x2f,
        B => 0x30,
        N => 0x31,
        M => 0x32,
        Comma => 0x33,
        Period => 0x34,
        Slash => 0x35,
        RightShift => 0x36,
        Alt => 0x38,
        Space => 0x39,
        F1 => 0x3b,
        F2 => 0x3c,
        F3 => 0x3d,
        F4 => 0x3e,
        F5 => 0x3f,
        F6 => 0x40,
        F7 => 0x41,
        F8 => 0x42,
        F9 => 0x43,
        F10 => 0x44,
        IntlBackslash => 0x56,
        F11 => 0x57,
        F12 => 0x58,
        Home => return (0x47, true),
        ArrowUp => return (0x48, true),
        PageUp => return (0x49, true),
        ArrowLeft => return (0x4b, true),
        ArrowRight => return (0x4d, true),
        End => return (0x4f, true),
        ArrowDown => return (0x50, true),
        PageDown => return (0x51, true),
        Delete => return (0x53, true),
        Meta => return (0x5b, true),
    };
    (ordinary, false)
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn linux_keycode(key: NativeKey) -> u16 {
    use NativeKey::*;
    match key {
        Home => 102,
        ArrowUp => 103,
        PageUp => 104,
        ArrowLeft => 105,
        ArrowRight => 106,
        End => 107,
        ArrowDown => 108,
        PageDown => 109,
        Delete => 111,
        Meta => 125,
        other => pc_scancode(other).0,
    }
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn x11_keysym(key: NativeKey) -> u32 {
    use NativeKey::*;
    match key {
        A => b'a' as u32,
        B => b'b' as u32,
        C => b'c' as u32,
        D => b'd' as u32,
        E => b'e' as u32,
        F => b'f' as u32,
        G => b'g' as u32,
        H => b'h' as u32,
        I => b'i' as u32,
        J => b'j' as u32,
        K => b'k' as u32,
        L => b'l' as u32,
        M => b'm' as u32,
        N => b'n' as u32,
        O => b'o' as u32,
        P => b'p' as u32,
        Q => b'q' as u32,
        R => b'r' as u32,
        S => b's' as u32,
        T => b't' as u32,
        U => b'u' as u32,
        V => b'v' as u32,
        W => b'w' as u32,
        X => b'x' as u32,
        Y => b'y' as u32,
        Z => b'z' as u32,
        Num0 => b'0' as u32,
        Num1 => b'1' as u32,
        Num2 => b'2' as u32,
        Num3 => b'3' as u32,
        Num4 => b'4' as u32,
        Num5 => b'5' as u32,
        Num6 => b'6' as u32,
        Num7 => b'7' as u32,
        Num8 => b'8' as u32,
        Num9 => b'9' as u32,
        Space => b' ' as u32,
        Minus => b'-' as u32,
        Equal => b'=' as u32,
        LeftBracket => b'[' as u32,
        RightBracket => b']' as u32,
        Backslash | IntlBackslash => b'\\' as u32,
        Semicolon => b';' as u32,
        Quote => b'\'' as u32,
        Comma => b',' as u32,
        Period => b'.' as u32,
        Slash => b'/' as u32,
        Grave => b'`' as u32,
        Backspace => 0xff08,
        Tab => 0xff09,
        Enter => 0xff0d,
        Escape => 0xff1b,
        Home => 0xff50,
        ArrowLeft => 0xff51,
        ArrowUp => 0xff52,
        ArrowRight => 0xff53,
        ArrowDown => 0xff54,
        PageUp => 0xff55,
        PageDown => 0xff56,
        End => 0xff57,
        Delete => 0xffff,
        F1 => 0xffbe,
        F2 => 0xffbf,
        F3 => 0xffc0,
        F4 => 0xffc1,
        F5 => 0xffc2,
        F6 => 0xffc3,
        F7 => 0xffc4,
        F8 => 0xffc5,
        F9 => 0xffc6,
        F10 => 0xffc7,
        F11 => 0xffc8,
        F12 => 0xffc9,
        LeftShift => 0xffe1,
        RightShift => 0xffe2,
        Control => 0xffe3,
        Alt => 0xffe9,
        Meta => 0xffeb,
    }
}

pub(super) fn is_modifier(key: NativeKey) -> bool {
    matches!(
        key,
        NativeKey::LeftShift
            | NativeKey::RightShift
            | NativeKey::Control
            | NativeKey::Alt
            | NativeKey::Meta
    )
}

/// Release ordinary keys first to avoid forming shortcuts during cleanup.
pub(super) fn release_order(held: &[NativeKey]) -> Vec<NativeKey> {
    held.iter()
        .copied()
        .filter(|k| !is_modifier(*k))
        .chain(held.iter().copied().filter(|k| is_modifier(*k)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_uses_extended_pc_codes_and_distinct_linux_codes() {
        assert_eq!(pc_scancode(NativeKey::ArrowLeft), (0x4b, true));
        assert_eq!(linux_keycode(NativeKey::ArrowLeft), 105);
        assert_eq!(pc_scancode(NativeKey::Backspace), (0x0e, false));
        assert_eq!(linux_keycode(NativeKey::Delete), 111);
        assert_eq!(mac_keycode(NativeKey::ArrowLeft), 123);
        assert_eq!(x11_keysym(NativeKey::ArrowLeft), 0xff51);
    }

    #[test]
    fn iso_and_modifier_keys_are_distinct() {
        assert_ne!(
            pc_scancode(NativeKey::IntlBackslash),
            pc_scancode(NativeKey::Backslash)
        );
        assert_ne!(
            mac_keycode(NativeKey::LeftShift),
            mac_keycode(NativeKey::RightShift)
        );
        assert_ne!(
            x11_keysym(NativeKey::LeftShift),
            x11_keysym(NativeKey::RightShift)
        );
    }

    #[test]
    fn cleanup_orders_keys_before_modifiers() {
        assert_eq!(
            release_order(&[
                NativeKey::Control,
                NativeKey::A,
                NativeKey::LeftShift,
                NativeKey::B
            ]),
            vec![
                NativeKey::A,
                NativeKey::B,
                NativeKey::Control,
                NativeKey::LeftShift
            ]
        );
    }
}
