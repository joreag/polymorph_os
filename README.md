# 🌐 PolymorphOS: The Sovereign Execution Engine (v0.5 Review Candidate)

[![License: BOKRLv2](https://img.shields.io/badge/License-BOKRLv2-blue.svg)](LICENSE.md)
[![Rust: no_std](https://img.shields.io/badge/Rust-no__std-orange.svg)]()
[![Status: v0.5 Review Candidate](https://img.shields.io/badge/Status-v0.5_Review_Candidate-purple.svg)]()

**A 300-millisecond booting, `#![no_std]` bare-metal kernel built from scratch in Rust. PolymorphOS is the foundational execution sandbox for the upcoming GenesisOS Universal Agentic Workspace.**

---

## 🛑 The Problem
The AI industry is currently racing to give autonomous agents (LLMs) "tools"—the ability to execute code, manage files, and interact with operating systems. 

Running these agents directly on host machines is extremely dangerous, and standard Linux Docker containers are heavy, slow, and susceptible to escapes. Existing operating systems were built in the 1990s for humans. They were not designed for the speed, structure, or security risks of Artificial General Intelligence.

## ⚡ The PolymorphOS Solution
**PolymorphOS is not a Linux distribution. It is a completely custom, sovereign operating system.** 

It runs on bare-metal silicon (via QEMU or native UEFI hardware). It provides a mathematically verified, Zero-Trust physical "Clean Room" where an external AI can write files, execute logic, and interact with memory without *any* physical connection to your host machine's file system or network stack.

*Note: This v0.5 release represents the Bare-Metal Kernel. The secure Agentic Gateway (GenesisOS) that provides internet routing and OMZTA Cryptography is currently in active development and will be merged in V1.*

---

## 🧠 Key Innovations & Architecture (Included in v0.5)

### 1. MICT-Elastic Memory Allocator
Standard OS allocators (Linked-Lists/Buddy Systems) use $O(N)$ or $O(\log N)$ searches, causing non-deterministic latency spikes and deadlocks during hardware interrupts. PolymorphOS utilizes a custom **Topological Heatmap Allocator** tied to L2 cache lines, resulting in lock-free, deterministic **$O(1)$ memory allocation**. 

### 2. Sovereign Storage Engine (MICT File System)
PolymorphOS features a scratch-built, zero-dependency PCIe NVMe driver. It bypasses traditional file-system bloat to execute Direct Memory Access (DMA) reads/writes directly to the flash controller.

### 3. The Semantic Desktop (3D Gaussian Splat UI)
Instead of a legacy 2D window manager, the PolymorphOS UI elements are composed of mathematically rendered 3D Gaussian Splats via the UEFI Graphics Output Protocol (GOP). Windows are organically alpha-blended "clouds" that dynamically coalesce, completely eliminating tearing via a 1-millisecond Double-Buffered compositor.

### 4. Lock-Free Asynchronous Execution
The kernel features a custom async waker and executor, polling tasks from a lock-free `crossbeam_queue` and dropping CPU load to 0% (via `hlt`) when idle.

---

## ⚖️ What This IS and What it IS NOT

*   **IT IS NOT** POSIX compliant. You cannot run `apt-get` or standard Linux binaries here. 
*   **IT IS NOT** a daily driver for web browsing. (The kernel intentionally lacks a standard TCP/IP stack to prevent autonomous network exploits).
*   **IT IS** a research-grade "Clean Room."
*   **IT IS** an educational masterclass in modern `#![no_std]` Rust hardware orchestration, PCIe enumeration, and interrupt handling.

---

## 🛠️ Quickstart

### Prerequisites
*   Rust Nightly toolchain (`rustup default nightly`)
*   QEMU (`qemu-system-x86_64` installed in your PATH)

### Booting the Kernel
Clone the repository and run:

Linux/Mac: ./run.sh
Windows: run.bat

### Hardware Deployment

### 1. Build the Release Image
First, compile the kernel with full optimizations. Physical hardware needs the speed.

```bash
cargo bootimage --release
```
*This generates a raw, bootable disk image located at: `target/x86_64-polymorph_os/release/bootimage-polymorph_os.bin`*

### 2. Identify the USB Drive
Plug in your USB drive and run `lsblk` to find its device path.

```bash
lsblk
```
Look at the output and identify your USB drive by its size (e.g., `sdb`, `sdc`, `nvme0n1`). **Do not guess.** If you pick your main OS drive, the next command will obliterate your Linux installation.

### 3. Flash to USB (The `dd` Command)
Assuming your USB drive is `/dev/sdX` (replace `X` with your actual drive letter, like `sdb`. **Do not include the partition number** like `sdb1`, just the drive `sdb`).

Run the `dd` command to bite-copy the kernel directly to the drive's boot sector:

```bash
sudo dd if=target/x86_64-polymorph_os/release/bootimage-polymorph_os.bin of=/dev/sdX bs=4M status=progress && sync
```

**Command Breakdown:**
*   `if=...`: **I**nput **F**ile (Our compiled bare-metal kernel).
*   `of=/dev/sdX`: **O**utput **F**ile (Your physical USB drive).
*   `bs=4M`: **B**lock **S**ize. Writes in 4MB chunks for significantly faster flashing.
*   `status=progress`: Shows a progress bar so you aren't staring at a blank terminal.
*   `&& sync`: **Crucial.** Forces Linux to flush all write caches to the USB drive before the command finishes, ensuring you don't corrupt the kernel when you unplug it.

Once that finishes, you can pull the USB drive out, plug it into a laptop, hit F12 to enter the boot menu, and watch your 3D Gaussian Splats render on bare metal!

*PolymorphOS will compile natively for `x86_64-unknown-none`, fuse with the OVMF UEFI firmware, and launch QEMU in < 300ms.*

**Available Local Commands:**
Click into the QEMU window to access the Semantic Terminal. 
Type `HELP` to view the currently active bare-metal commands (e.g., `SCAN PCI`, `PING NVME`, `LIST`).
COMANDS NOT AVAILABLE YET: DELETE, MOVE(Coming next with the expanded MFS) 

---

## 🔮 Future Roadmap

### Phase 1: The GenesisOS Integration (V1 Full Release)
This kernel is designed to act as the "Subconscious Engine" to the GenesisOS "Conscious Gateway." V1 will merge this repository with our Tauri/React Host OS, introducing:
*   **The TCP Umbilical Cord:** Routing commands from the Host to the Kernel via isolated serial pipelines.
*   **OMZTA Cryptography:** Mandating `ed25519` cryptographic signatures for all LLM-generated code before bare-metal execution.
*   **Live AI Interaction:** Seamlessly executing LLM prompts directly into the bare-metal environment.

### Phase 2: Advanced Cognition
*   **JARVITS Integration:** Natively hosting the Pascal-Chimera v9 Cognitive Architecture.
*   **MDO Compilation:** Dynamic runtime compilation of `.mdo` (MICT Data Object) logic modules directly to LLVM IR, bypassing Rust entirely for native execution.

---
**License:** [Boredbrains Open Knowledge Return License v2 (BOKRLv2)](LICENSE.md)  
*Built by John Edward Reagan III & the Boredbrains Consortium.*

***

