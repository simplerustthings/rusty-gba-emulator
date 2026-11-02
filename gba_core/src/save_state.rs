pub trait SaveState: Sized {
    fn save(&self, buf: &mut Vec<u8>);
    fn load(buf: &mut &[u8]) -> Self;
}

macro_rules! impl_save_state_primitive {
    ($ty:ty, $read:ident, $write:ident, $size:expr) => {
        impl SaveState for $ty {
            fn save(&self, buf: &mut Vec<u8>) {
                buf.extend_from_slice(&self.$write());
            }
            fn load(buf: &mut &[u8]) -> Self {
                let mut bytes = [0; $size];
                bytes.copy_from_slice(&buf[..$size]);
                *buf = &buf[$size..];
                <$ty>::$read(bytes)
            }
        }
    };
}

impl_save_state_primitive!(u8, from_le_bytes, to_le_bytes, 1);
impl_save_state_primitive!(u16, from_le_bytes, to_le_bytes, 2);
impl_save_state_primitive!(u32, from_le_bytes, to_le_bytes, 4);
impl_save_state_primitive!(u64, from_le_bytes, to_le_bytes, 8);
impl_save_state_primitive!(i8, from_le_bytes, to_le_bytes, 1);
impl_save_state_primitive!(i16, from_le_bytes, to_le_bytes, 2);
impl_save_state_primitive!(i32, from_le_bytes, to_le_bytes, 4);
impl_save_state_primitive!(i64, from_le_bytes, to_le_bytes, 8);

impl SaveState for bool {
    fn save(&self, buf: &mut Vec<u8>) {
        buf.push(*self as u8);
    }
    fn load(buf: &mut &[u8]) -> Self {
        let val = buf[0];
        *buf = &buf[1..];
        val != 0
    }
}

impl SaveState for usize {
    fn save(&self, buf: &mut Vec<u8>) {
        (*self as u64).save(buf);
    }
    fn load(buf: &mut &[u8]) -> Self {
        u64::load(buf) as usize
    }
}

impl<T: SaveState> SaveState for Vec<T> {
    fn save(&self, buf: &mut Vec<u8>) {
        (self.len() as u64).save(buf);
        for item in self.iter() {
            item.save(buf);
        }
    }
    fn load(buf: &mut &[u8]) -> Self {
        let len = u64::load(buf) as usize;
        let mut v = Vec::with_capacity(len);
        for _ in 0..len {
            v.push(T::load(buf));
        }
        v
    }
}

impl<T: SaveState> SaveState for Box<[T]> {
    fn save(&self, buf: &mut Vec<u8>) {
        (self.len() as u64).save(buf);
        for item in self.iter() {
            item.save(buf);
        }
    }
    fn load(buf: &mut &[u8]) -> Self {
        let len = u64::load(buf) as usize;
        let mut v = Vec::with_capacity(len);
        for _ in 0..len {
            v.push(T::load(buf));
        }
        v.into_boxed_slice()
    }
}

impl<T: SaveState, const N: usize> SaveState for [T; N] {
    fn save(&self, buf: &mut Vec<u8>) {
        for item in self.iter() {
            item.save(buf);
        }
    }
    fn load(buf: &mut &[u8]) -> Self {
        std::array::from_fn(|_| T::load(buf))
    }
}

impl<T: SaveState> SaveState for Option<T> {
    fn save(&self, buf: &mut Vec<u8>) {
        match self {
            Some(val) => {
                buf.push(1);
                val.save(buf);
            }
            None => {
                buf.push(0);
            }
        }
    }
    fn load(buf: &mut &[u8]) -> Self {
        if buf[0] != 0 {
            *buf = &buf[1..];
            Some(T::load(buf))
        } else {
            *buf = &buf[1..];
            None
        }
    }
}

/// Implement SaveState for a struct by listing its fields in order.
/// Usage: impl_save_state_fields!(Foo { field1, field2, field3 });
#[macro_export]
macro_rules! impl_save_state_fields {
    ($name:ident { $($field:ident),+ $(,)? }) => {
        impl $crate::save_state::SaveState for $name {
            fn save(&self, buf: &mut Vec<u8>) {
                $($crate::save_state::SaveState::save(&self.$field, buf);)+
            }
            fn load(buf: &mut &[u8]) -> Self {
                $name {
                    $($field: $crate::save_state::SaveState::load(buf),)+
                }
            }
        }
    };
}

/// Implement SaveState for a tuple struct (newtype wrapper).
#[macro_export]
macro_rules! impl_save_state_tuple {
    ($name:ident) => {
        impl $crate::save_state::SaveState for $name {
            fn save(&self, buf: &mut Vec<u8>) {
                $crate::save_state::SaveState::save(&self.0, buf);
            }
            fn load(buf: &mut &[u8]) -> Self {
                $name($crate::save_state::SaveState::load(buf))
            }
        }
    };
}

/// Implement SaveState for a C-like enum (no data in any variant).
/// Usage: impl_save_state_enum!(MyEnum { Variant1, Variant2, Variant3 });
#[macro_export]
macro_rules! impl_save_state_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        impl $crate::save_state::SaveState for $name {
            fn save(&self, buf: &mut Vec<u8>) {
                let discriminant: u8 = match self {
                    $(Self::$variant => $name::$variant as u8,)+
                };
                buf.push(discriminant);
            }
            fn load(buf: &mut &[u8]) -> Self {
                let discriminant = buf[0];
                *buf = &buf[1..];
                match discriminant {
                    $(
                        d if d == $name::$variant as u8 => Self::$variant,
                    )+
                    _ => panic!("Unknown discriminant for {}: {}", stringify!($name), discriminant),
                }
            }
        }
    };
}
