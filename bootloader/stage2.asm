; Ralph OS Stage 2 Bootloader
; Loaded at 0x7E00 by stage 1
; Transitions from 16-bit real mode to 64-bit long mode

[BITS 16]
[ORG 0x7E00]

KERNEL_ADDR         equ 0x100000    ; Load kernel at 1MB
KERNEL_SECTORS      equ 64          ; 32KB kernel max (for now)
KERNEL_LOAD_SEG     equ 0x1000      ; Temporary load segment (0x10000)
KERNEL_LOAD_OFF     equ 0x0000

; Page table locations (must be 4KB aligned)
PML4_ADDR           equ 0x1000
PDPT_ADDR           equ 0x2000
PD_ADDR             equ 0x3000
PT_ADDR             equ 0x4000

start:
    ; We're in 16-bit real mode, loaded at 0x7E00
    ; DL contains boot drive from stage 1
    mov [boot_drive], dl

    ; Print stage 2 message
    mov si, msg_stage2
    call print_string

    ; Enable A20 line
    call enable_a20

    ; Load kernel from disk (still in real mode, need BIOS)
    call load_kernel

    ; Set up GDT
    lgdt [gdt_descriptor]

    ; Disable interrupts
    cli

    ; Switch to 32-bit protected mode
    mov eax, cr0
    or eax, 1               ; Set PE bit
    mov cr0, eax

    ; Far jump to 32-bit code
    jmp 0x08:protected_mode

; ============================================================================
; 16-bit helper functions
; ============================================================================

enable_a20:
    ; Try keyboard controller method
    mov si, msg_a20
    call print_string

    call .wait_input
    mov al, 0xAD            ; Disable keyboard
    out 0x64, al

    call .wait_input
    mov al, 0xD0            ; Read output port
    out 0x64, al

    call .wait_output
    in al, 0x60
    push ax

    call .wait_input
    mov al, 0xD1            ; Write output port
    out 0x64, al

    call .wait_input
    pop ax
    or al, 2                ; Set A20 bit
    out 0x60, al

    call .wait_input
    mov al, 0xAE            ; Enable keyboard
    out 0x64, al

    call .wait_input

    mov si, msg_ok
    call print_string
    ret

.wait_input:
    in al, 0x64
    test al, 2
    jnz .wait_input
    ret

.wait_output:
    in al, 0x64
    test al, 1
    jz .wait_output
    ret

load_kernel:
    mov si, msg_kernel
    call print_string

    ; Load kernel to temporary location (below 1MB, BIOS limitation)
    ; We'll copy it to 1MB after entering protected mode
    mov ax, KERNEL_LOAD_SEG
    mov es, ax
    mov bx, KERNEL_LOAD_OFF

    ; Read sectors using extended BIOS function (for >64KB)
    mov ah, 0x02            ; Read sectors
    mov al, KERNEL_SECTORS  ; Number of sectors
    mov ch, 0               ; Cylinder 0
    mov cl, 18              ; Sector 18 (after stage1 + stage2: 1 + 16 = 17)
    mov dh, 0               ; Head 0
    mov dl, [boot_drive]
    int 0x13
    jc .error

    mov si, msg_ok
    call print_string

    ; Reset ES
    xor ax, ax
    mov es, ax
    ret

.error:
    mov si, msg_disk_error
    call print_string
    jmp halt16

halt16:
    cli
    hlt
    jmp halt16

print_string:
    push ax
    push bx
    mov ah, 0x0E
    mov bh, 0
.loop:
    lodsb
    test al, al
    jz .done
    int 0x10
    jmp .loop
.done:
    pop bx
    pop ax
    ret

; ============================================================================
; 32-bit protected mode code
; ============================================================================

[BITS 32]

protected_mode:
    ; Set up segment registers for protected mode
    mov ax, 0x10            ; Data segment selector
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    mov esp, 0x90000        ; Set up stack

    ; Copy kernel from temporary location to 1MB
    mov esi, (KERNEL_LOAD_SEG << 4) + KERNEL_LOAD_OFF
    mov edi, KERNEL_ADDR
    mov ecx, (KERNEL_SECTORS * 512) / 4     ; Copy dwords
    rep movsd

    ; Set up page tables for long mode (identity mapping first 2MB)
    call setup_page_tables

    ; Enable PAE (Physical Address Extension)
    mov eax, cr4
    or eax, (1 << 5)        ; Set PAE bit
    mov cr4, eax

    ; Load PML4 address into CR3
    mov eax, PML4_ADDR
    mov cr3, eax

    ; Enable long mode in EFER MSR
    mov ecx, 0xC0000080     ; EFER MSR
    rdmsr
    or eax, (1 << 8)        ; Set LME bit
    wrmsr

    ; Enable paging (this activates long mode)
    mov eax, cr0
    or eax, (1 << 31)       ; Set PG bit
    mov cr0, eax

    ; Far jump to 64-bit code
    jmp 0x18:long_mode

setup_page_tables:
    ; Clear page table memory
    mov edi, PML4_ADDR
    mov ecx, 4096           ; 4 pages worth
    xor eax, eax
    rep stosd

    ; PML4[0] -> PDPT
    mov dword [PML4_ADDR], PDPT_ADDR | 0x03     ; Present + Writable

    ; PDPT[0] -> PD
    mov dword [PDPT_ADDR], PD_ADDR | 0x03       ; Present + Writable

    ; PD[0] -> 2MB page (use huge pages for simplicity)
    ; Actually, let's map first 2MB using 2MB huge page
    mov dword [PD_ADDR], 0x00000083             ; Present + Writable + Huge (2MB)

    ; Map more memory for kernel at 1MB
    ; PD[1] -> another 2MB page (covers 2MB-4MB)
    mov dword [PD_ADDR + 8], 0x00200083         ; 2MB + flags

    ret

; ============================================================================
; 64-bit long mode code
; ============================================================================

[BITS 64]

long_mode:
    ; Set up segment registers (mostly unused in long mode, but clear them)
    mov ax, 0x20            ; 64-bit data segment
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    ; Set up stack
    mov rsp, 0x90000

    ; Jump to kernel at 1MB
    mov rax, KERNEL_ADDR
    jmp rax

; ============================================================================
; Data
; ============================================================================

boot_drive:     db 0
msg_stage2:     db "Stage 2: ", 0
msg_a20:        db "A20 line...", 0
msg_kernel:     db "Loading kernel...", 0
msg_ok:         db " OK", 13, 10, 0
msg_disk_error: db " DISK ERROR!", 13, 10, 0

; ============================================================================
; GDT (Global Descriptor Table)
; ============================================================================

align 8
gdt_start:
    ; Null descriptor (required)
    dq 0

    ; 32-bit code segment (0x08)
    dw 0xFFFF               ; Limit (low)
    dw 0x0000               ; Base (low)
    db 0x00                 ; Base (middle)
    db 10011010b            ; Access: Present, Ring 0, Code, Executable, Readable
    db 11001111b            ; Flags: 4KB granularity, 32-bit + Limit (high)
    db 0x00                 ; Base (high)

    ; 32-bit data segment (0x10)
    dw 0xFFFF               ; Limit (low)
    dw 0x0000               ; Base (low)
    db 0x00                 ; Base (middle)
    db 10010010b            ; Access: Present, Ring 0, Data, Writable
    db 11001111b            ; Flags: 4KB granularity, 32-bit + Limit (high)
    db 0x00                 ; Base (high)

    ; 64-bit code segment (0x18)
    dw 0x0000               ; Limit (ignored in long mode)
    dw 0x0000               ; Base (low)
    db 0x00                 ; Base (middle)
    db 10011010b            ; Access: Present, Ring 0, Code, Executable, Readable
    db 00100000b            ; Flags: Long mode
    db 0x00                 ; Base (high)

    ; 64-bit data segment (0x20)
    dw 0x0000               ; Limit (ignored)
    dw 0x0000               ; Base (low)
    db 0x00                 ; Base (middle)
    db 10010010b            ; Access: Present, Ring 0, Data, Writable
    db 00000000b            ; Flags
    db 0x00                 ; Base (high)

gdt_end:

gdt_descriptor:
    dw gdt_end - gdt_start - 1      ; Size
    dd gdt_start                     ; Address

; End marker for size calculation
stage2_end:
