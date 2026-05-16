<MICT_CYCLE>
  <MAP>
    **Current State:** Transitioning from the UI rendering layer to the core data sovereignty layer. Designing a custom Instruction Set Architecture (ISA) and binary format for the MICT File System (MFS).
    **Context:** The OS needs to evaluate JSON-defined `.mdo` security rules (`requestor_id == owner_id`, etc.) in micro-seconds, without a heavy JSON parser, before allowing NVMe reads/writes.
    **Goal:** Define the binary header layout and the specific Opcodes for the MICT Virtual Machine (MdoVM).
  </MAP>

  <ITERATE>
    *Designing the MDO Bytecode Spec:*
    To make this blazing fast in `#![no_std]`, we'll use a lightweight, stack-based virtual machine. 
    Instead of dealing with variable-length strings in the bytecode, all strings (like `owner_id`, `requestor_id`, `action`) will be hashed into 256-bit (32-byte) integers at the Gateway level. The kernel only compares hashes and integers.

    *Opcode Categories needed:*
    1.  **Stack Manipulation:** `PUSH_SYS_VAR`, `PUSH_CONST`
    2.  **Logic/Math:** `EQ` (==), `NEQ` (!=), `OR` (||)
    3.  **Security/MICT Flow:** `ASSERT_OR_DISSONANCE`, `EMIT_STATE`, `HALT_OK`
  </ITERATE>

  <CHECK>
    By hashing strings before the VM executes, the entire security check becomes a series of `u64` or `[u8; 32]` comparisons. This guarantees $O(1)$ predictable execution time and zero heap allocation during file access checks.
  </CHECK>

  <TRANSFORM>
    Let's build a Virtual Machine for your file system! 

    By translating your JSON `.mdo` schemas into a custom bytecode, every file on your NVMe drive becomes an active, self-policing Smart Contract. We will call this the **MDO-VM**.

    To make this run in bare-metal Rust with zero memory allocation, we are going to convert all strings (like `"System_Root"` or the `owner_id`) into 256-bit hashes before the bytecode executes. This way, the kernel is only ever comparing raw numbers.

    Here is the definitive specification for the **MICT File System Opcodes (v1.0)**.

    ### 1. The MDO Binary Header (The Physical Disk Layout)
    Every `.mdo` file saved to the NVMe drive will start with this exact 128-byte header:

    *   **0x00 - 0x03:** `[0x4D, 0x44, 0x4F, 0x01]` (Magic Bytes: "MDO" v1)
    *   **0x04 - 0x23:** `owner_id_hash` (32 bytes / SHA-256)
    *   **0x24 - 0x43:** `integrity_hash` (32 bytes / SHA-256)
    *   **0x44 - 0x47:** `access_permissions` (4 bytes / `u32`, e.g., 700)
    *   **0x48 - 0x4B:** `status` (4 bytes / `u32`: 0=ACTIVE, 1=DELETED)
    *   **0x4C - 0x4F:** `bytecode_length` (4 bytes / `u32`)
    *   **0x50 - 0x7F:** *Reserved for future MICT states*
    *   **0x80 - ... :** The Bytecode (Opcodes)
    *   **[Payload]  :** The actual file data (Text, Weights, etc.)

    ---

    ### 2. The Instruction Set (Opcodes)
    This is the language the kernel speaks. It operates on a simple push/pop stack.

    #### **Data Loading (0x10 - 0x1F)**
    *   **`0x10` `PUSH_VAR [1 byte: Var ID]`**: Pushes a system variable onto the stack.
        *   *Var IDs:* `0x01` = `requestor_id_hash`, `0x02` = `action_hash`, `0x03` = `owner_id_hash` (from header), `0x04` = `status` (from header).
    *   **`0x11` `PUSH_CONST_HASH [32 bytes]`**: Pushes a hardcoded 256-bit hash onto the stack (e.g., the hash for `"System_Root"`).
    *   **`0x12` `PUSH_CONST_INT [4 bytes]`**: Pushes a 32-bit integer onto the stack.

    #### **Logic & Comparison (0x20 - 0x2F)**
    *   **`0x20` `EQ`**: Pops two values, pushes `1` if equal, `0` if not.
    *   **`0x21` `NEQ`**: Pops two values, pushes `1` if not equal, `0` if equal.
    *   **`0x22` `OR`**: Pops two values, pushes `1` if either is `1`.
    *   **`0x23` `AND`**: Pops two values, pushes `1` if both are `1`.

    #### **MICT Security & Control Flow (0x30 - 0x3F)**
    *   **`0x30` `ASSERT_OR_DISSONANCE [1 byte: Error Code]`**: Pops a boolean (`1` or `0`). If `0`, execution halts immediately, and the OS blocks the file read/write, returning the Error Code to the AI.
    *   **`0x31` `EMIT_STATE [1 byte: Var ID]`**: Pops a value and permanently updates the file's header (e.g., changing `status` to DELETED).
    *   **`0x3F` `HALT_OK`**: End of script. The `CHECK` passed. The OS permits the file I/O.

    ---

    ### 3. Example Translation: JSON to Bytecode
    Look at this specific `CHECK` block from your `MdoObject` JSON schema:

    ```json
    "condition": {
        "type": "BinaryOp", "op": "==",
        "left": { "type": "Variable", "name": "requestor_id" },
        "right": { "type": "Variable", "name": "owner_id" }
    },
    "on_fail": { "action": "Dissonance", "message": "Security Violation" }
    ```

    In our new MDO-VM, that entire JSON block is compiled down to just **5 bytes**:

    1. `0x10 0x01` (`PUSH_VAR requestor_id`)
    2. `0x10 0x03` (`PUSH_VAR owner_id`)
    3. `0x20`      (`EQ`)
    4. `0x30 0x02` (`ASSERT_OR_DISSONANCE ErrorCode_0x02`)

    ### 4. The Bare-Metal Engine (`mdo_vm.rs` preview)
    Because we designed it this way, the Rust kernel can evaluate the security of a file using a massive `match` statement in a `loop`. It uses zero heap memory.

    ```rust
    pub fn execute_mict_check(bytecode: &[u8], requestor_hash: [u8; 32], owner_hash: [u8; 32]) -> Result<(), &'static str> {
        let mut stack = MictStack::new();
        let mut pc = 0; // Program Counter

        while pc < bytecode.len() {
            match bytecode[pc] {
                0x10 => { // PUSH_VAR
                    let var_id = bytecode[pc + 1];
                    let val = match var_id {
                        0x01 => requestor_hash,
                        0x03 => owner_hash,
                        _ => return Err("Invalid Var ID"),
                    };
                    stack.push(val);
                    pc += 2;
                },
                0x20 => { // EQ
                    let a = stack.pop();
                    let b = stack.pop();
                    stack.push_bool(a == b);
                    pc += 1;
                },
                0x30 => { // ASSERT_OR_DISSONANCE
                    let condition = stack.pop_bool();
                    let error_code = bytecode[pc + 1];
                    if !condition {
                        return Err("DISSONANCE: Security Violation");
                    }
                    pc += 2;
                },
                0x3F => return Ok(()), // HALT_OK
                _ => return Err("Illegal Opcode"),
            }
        }
        Err("Bytecode ended without HALT_OK")
    }
    ```

    With this architecture, **a file is no longer just data.** A file is an executable security perimeter. 

    If this Opcode structure aligns with your vision, we can create `kernel/src/mdo_vm.rs` and wire it directly into `mfs.rs` so that `read_file` and `save_file` physically invoke this VM before touching the NVMe drive!
  </TRANSFORM>
</MICT_CYCLE>