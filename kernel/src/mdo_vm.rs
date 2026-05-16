// kernel/src/mdo_vm.rs

/// The environmental context provided by GenesisOS when requesting file access.
/// All strings are pre-hashed (SHA-256) into 32-byte arrays.
#[derive(Debug, Clone, Copy)]
pub struct MdoContext {
    pub requestor_id_hash: [u8; 32],
    pub action_hash: [u8; 32],
    pub owner_id_hash: [u8; 32], 
    pub status: u32,
}

/// A zero-allocation, fixed-size stack for the MDO Virtual Machine.
/// Uses 1 KB of kernel stack memory.
struct MictStack {
    data: [[u8; 32]; 32],
    ptr: usize,
}

impl MictStack {
    #[inline(always)]
    fn new() -> Self {
        MictStack {
            data: [[0; 32]; 32],
            ptr: 0,
        }
    }

    #[inline(always)]
    fn push(&mut self, val: [u8; 32]) -> Result<(), &'static str> {
        if self.ptr >= 32 { return Err("MDO-VM Stack Overflow"); }
        self.data[self.ptr] = val;
        self.ptr += 1;
        Ok(())
    }

    #[inline(always)]
    fn pop(&mut self) -> Result<[u8; 32], &'static str> {
        if self.ptr == 0 { return Err("MDO-VM Stack Underflow"); }
        self.ptr -= 1;
        Ok(self.data[self.ptr])
    }

    #[inline(always)]
    fn push_bool(&mut self, b: bool) -> Result<(), &'static str> {
        let mut val = [0u8; 32];
        if b { val[0] = 1; }
        self.push(val)
    }

    #[inline(always)]
    fn pop_bool(&mut self) -> Result<bool, &'static str> {
        let val = self.pop()?;
        Ok(val[0] != 0)
    }
}

// --- MICT OPCODES ---
const OP_PUSH_VAR: u8 = 0x10;
const OP_PUSH_CONST_HASH: u8 = 0x11;
const OP_EQ: u8 = 0x20;
const OP_NEQ: u8 = 0x21;
const OP_ASSERT_OR_DISSONANCE: u8 = 0x30;
const OP_HALT_OK: u8 = 0x3F;

// --- SYSTEM VARIABLE IDs ---
const VAR_REQUESTOR_ID: u8 = 0x01;
const VAR_ACTION: u8 = 0x02;
const VAR_OWNER_ID: u8 = 0x03;
const VAR_STATUS: u8 = 0x04;

/// Executes the MICT File System Bytecode.
/// Returns Ok(()) if the assertions pass, or Err(Error Code) if a Dissonance is thrown.
pub fn execute_mict_check(bytecode: &[u8], ctx: &MdoContext) -> Result<(), u8> {
    let mut stack = MictStack::new();
    let mut pc = 0; // Program Counter

    while pc < bytecode.len() {
        match bytecode[pc] {
            OP_PUSH_VAR => {
                if pc + 1 >= bytecode.len() { return Err(0xFF); } // 0xFF = Malformed Bytecode
                let var_id = bytecode[pc + 1];
                let mut val = [0u8; 32];

                match var_id {
                    VAR_REQUESTOR_ID => val = ctx.requestor_id_hash,
                    VAR_ACTION => val = ctx.action_hash,
                    VAR_OWNER_ID => val = ctx.owner_id_hash,
                    VAR_STATUS => {
                        // Pad the 32-bit integer into the 32-byte array
                        val[0..4].copy_from_slice(&ctx.status.to_le_bytes());
                    },
                    _ => return Err(0xFE), // 0xFE = Unknown Variable
                }

                if stack.push(val).is_err() { return Err(0xFD); } // 0xFD = Stack Error
                pc += 2;
            },

            OP_PUSH_CONST_HASH => {
                if pc + 32 >= bytecode.len() { return Err(0xFF); }
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&bytecode[pc + 1 .. pc + 33]);
                
                if stack.push(hash).is_err() { return Err(0xFD); }
                pc += 33;
            },

            OP_EQ => {
                let a = stack.pop().map_err(|_| 0xFD)?;
                let b = stack.pop().map_err(|_| 0xFD)?;
                stack.push_bool(a == b).map_err(|_| 0xFD)?;
                pc += 1;
            },

            OP_NEQ => {
                let a = stack.pop().map_err(|_| 0xFD)?;
                let b = stack.pop().map_err(|_| 0xFD)?;
                stack.push_bool(a != b).map_err(|_| 0xFD)?;
                pc += 1;
            },

            OP_ASSERT_OR_DISSONANCE => {
                if pc + 1 >= bytecode.len() { return Err(0xFF); }
                let error_code = bytecode[pc + 1];
                let condition = stack.pop_bool().map_err(|_| 0xFD)?;
                
                if !condition {
                    // [DISSONANCE TRIGGERED] The file actively rejects the operation!
                    return Err(error_code); 
                }
                pc += 2;
            },

            OP_HALT_OK => {
                // The script reached a valid, verified conclusion. Access Granted.
                return Ok(());
            },

            _ => return Err(0xFC), // 0xFC = Illegal Instruction
        }
    }

    // If we run out of bytecode without hitting HALT_OK, reject by default (Fail-Safe)
    Err(0xFB) 
}