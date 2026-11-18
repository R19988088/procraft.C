//! Records as the app hands them over and takes them back: fields one after another, each a 16-bit
//! id, a 32-bit length and that many bytes, all little-endian.

/// A record being written.
#[derive(Default)]
pub struct Record(Vec<u8>);

impl Record {
    pub fn new() -> Record {
        Record(Vec::new())
    }

    /// Field `id` holding `bytes`.
    pub fn field(&mut self, id: u16, bytes: &[u8]) -> &mut Record {
        if let Ok(len) = u32::try_from(bytes.len()) {
            self.0.extend_from_slice(&id.to_le_bytes());
            self.0.extend_from_slice(&len.to_le_bytes());
            self.0.extend_from_slice(bytes);
        }
        self
    }

    pub fn byte(&mut self, id: u16, value: u8) -> &mut Record {
        self.field(id, &[value])
    }

    pub fn word(&mut self, id: u16, value: u32) -> &mut Record {
        self.field(id, &value.to_le_bytes())
    }

    pub fn real(&mut self, id: u16, value: f32) -> &mut Record {
        self.field(id, &value.to_le_bytes())
    }

    pub fn bytes(self) -> Vec<u8> {
        self.0
    }
}

/// The fields of the record `bytes`, in order; none from one cut short on.
pub fn fields(bytes: &[u8]) -> impl Iterator<Item = (u16, &[u8])> {
    let mut rest = bytes;
    core::iter::from_fn(move || {
        let (head, after) = rest.split_at_checked(6)?;
        let id = u16::from_le_bytes([head[0], head[1]]);
        let len = u32::from_le_bytes([head[2], head[3], head[4], head[5]]) as usize;
        let (value, next) = after.split_at_checked(len)?;
        rest = next;
        Some((id, value))
    })
}

pub fn u32_of(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(*bytes.first_chunk::<4>()?))
}

pub fn u64_of(bytes: &[u8]) -> Option<u64> {
    Some(u64::from_le_bytes(*bytes.first_chunk::<8>()?))
}

pub fn f32_of(bytes: &[u8]) -> Option<f32> {
    Some(f32::from_le_bytes(*bytes.first_chunk::<4>()?)).filter(|v| v.is_finite())
}

/// The 32-bit floats `bytes` holds, as many as are whole.
pub fn f32s(bytes: &[u8]) -> Vec<f32> {
    bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect()
}
