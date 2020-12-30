// Copyright (c) SimpleStaking and Tezedge Contributors
// SPDX-License-Identifier: MIT

use redbpf_probes::{maps::{HashMap, RingBuffer}, helpers, registers::Registers};
use super::{data_descriptor::{SocketId, EventId, DataTag}, send};

#[derive(Clone)]
pub struct SyscallContextKey {
    pid: u32,
}

#[derive(Clone)]
pub struct SyscallContextFull {
    inner: SyscallContext,
    ts: u64,
}

#[derive(Clone)]
pub enum SyscallContext {
    Empty {
        fake_fd: u32,
        fake_data: &'static [u8],
    },

    Write {
        fd: u32,
        data_ptr: usize,
    },
    SendTo {
        fd: u32,
        data_ptr: usize,
    },
    SendMsg {
        fd: u32,
        message: &'static [u8],
    },

    Read {
        fd: u32,
        data_ptr: usize,
    },
    RecvFrom {
        fd: u32,
        data_ptr: usize,
    },

    Connect {
        fd: u32,
        address: &'static [u8],
    },
}

#[inline(always)]
fn e_unknown_fd(id: u64) -> EventId {
    let ts = helpers::bpf_ktime_get_ns();
    let socket_id = SocketId {
        pid: (id >> 32) as u32,
        fd: 0,
    };
    // same timestamp because event is instant
    EventId::new(socket_id, ts, ts)
}

impl SyscallContext {
    /// bpf validator forbids reading from stack uninitialized data
    /// different variants of this enum has different length,
    /// `Empty` variant should be biggest
    #[inline(always)]
    pub fn empty() -> Self {
        SyscallContext::Empty {
            fake_fd: 0,
            fake_data: b"",
        }
    }

    #[inline(always)]
    pub fn push(self, regs: &Registers, map: &mut HashMap<SyscallContextKey, SyscallContextFull>, rb: &mut RingBuffer) {
        let _ = regs;
        let id = helpers::bpf_get_current_pid_tgid();
        let key = SyscallContextKey {
            pid: (id & 0xffffffff) as u32,
        };
        if map.get(&key).is_some() {
            send::sized::<typenum::U8, typenum::B1>(e_unknown_fd(id), DataTag::Debug, 0xdeadbeef_u64.to_be_bytes().as_ref(), rb);
            map.delete(&key);
        } else {
            let s = SyscallContextFull {
                inner: self,
                ts: helpers::bpf_ktime_get_ns(),
            };
            map.set(&key, &s);
        }
    }

    #[inline(always)]
    pub fn pop_with<F>(regs: &Registers, map: &mut HashMap<SyscallContextKey, SyscallContextFull>, rb: &mut RingBuffer, f: F)
    where
        F: FnOnce(Self, u64),
    {
        let _ = regs;
        let id = helpers::bpf_get_current_pid_tgid();
        let key = SyscallContextKey {
            pid: (id & 0xffffffff) as u32,
        };
        match map.get(&key) {
            Some(context) => {
                f(context.inner.clone(), context.ts.clone());
                map.delete(&key);
            },
            None => (),
        }
    }
}
