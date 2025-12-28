; Ralph OS Stage 2 Bootloader
; Loaded at 0x7E00 by stage 1
; Transitions from 16-bit real mode to 64-bit long mode

[BITS 16]
[ORG 0x7E00]

KERNEL_ADDR         equ 0x100000    ; Load kernel at 1MB

; Kernel loading - use multiple small reads to avoid BIOS issues
; Each track has 18 sectors, reading within track boundaries is safest
KERNEL_LOAD_SEG     equ 0x1000      ; Load to 0x10000
KERNEL_SECTORS      equ 400         ; ~200KB max (kernel + exec table + programs)

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

    ; Check VGA debug flag and set mode 13h if enabled
    call check_vga_flag

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

; Check if VGA debug flag is set in the vga_flag variable
; If set, switch to VGA mode 13h (320x200x256)
; The vga_flag variable is at a fixed offset that can be patched by the Makefile
check_vga_flag:
    ; Debug: always print to verify this code runs
    mov si, msg_check_vga
    call print_string

    ; Read flag from our local variable
    mov al, [vga_flag]
    cmp al, 0x01
    jne .skip_vga

    ; Set VGA mode 13h
    mov si, msg_vga
    call print_string
    mov ah, 0x00
    mov al, 0x13            ; Mode 13h: 320x200x256
    int 0x10

    ; Store VGA status at 0x501 for kernel to read
    xor ax, ax
    mov es, ax
    mov byte [es:0x501], 0x13

    mov si, msg_ok
    call print_string

.skip_vga:
    ret

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

    ; Try LBA extensions first (INT 13h AH=42h)
    ; Check if extensions are available
    mov ah, 0x41
    mov bx, 0x55AA
    mov dl, [boot_drive]
    int 0x13
    jc .use_chs               ; Extensions not available
    cmp bx, 0xAA55
    jne .use_chs

    ; Use LBA mode - much simpler!
    mov word [dap_sectors], 8         ; Read 8 sectors at a time
    mov word [dap_buffer_off], 0
    mov word [dap_buffer_seg], KERNEL_LOAD_SEG
    mov dword [dap_lba_low], 17       ; Start at LBA 17 (sector 18)
    mov dword [dap_lba_high], 0
    mov word [sectors_left], KERNEL_SECTORS

.lba_loop:
    cmp word [sectors_left], 0
    je .done

    ; Adjust sectors to read if near end
    mov ax, [sectors_left]
    cmp ax, 8
    jae .read_8
    mov [dap_sectors], ax
    jmp .do_read
.read_8:
    mov word [dap_sectors], 8

.do_read:
    mov ah, 0x42
    mov dl, [boot_drive]
    mov si, dap
    int 0x13
    jc .lba_error

    ; Progress dot
    mov al, '.'
    mov ah, 0x0E
    int 0x10

    ; Update counters
    movzx ax, byte [dap_sectors]
    sub [sectors_left], ax

    ; Advance LBA
    add [dap_lba_low], eax

    ; Advance buffer
    movzx eax, byte [dap_sectors]
    shl ax, 9                   ; * 512
    add [dap_buffer_off], ax
    jnc .lba_loop
    ; Handle segment overflow
    add word [dap_buffer_seg], 0x1000
    mov word [dap_buffer_off], 0
    jmp .lba_loop

.done:
    mov si, msg_ok
    call print_string
    xor ax, ax
    mov es, ax
    ret

.lba_error:
    mov si, msg_disk_error
    call print_string
    mov al, ah
    shr al, 4
    call .print_hex_digit
    mov al, ah
    and al, 0x0F
    call .print_hex_digit
    jmp halt16

; Fallback: CHS mode (for systems without LBA support)
.use_chs:
    mov ax, KERNEL_LOAD_SEG
    mov es, ax
    xor bx, bx
    mov byte [cur_sector], 18
    mov byte [cur_head], 0
    mov byte [cur_cyl], 0
    mov word [sectors_left], KERNEL_SECTORS

.chs_loop:
    cmp word [sectors_left], 0
    je .done

    mov ah, 0x02
    mov al, 1
    mov ch, [cur_cyl]
    mov cl, [cur_sector]
    mov dh, [cur_head]
    mov dl, [boot_drive]
    int 0x13
    jc .chs_error

    dec word [sectors_left]
    add bx, 512
    jnc .no_overflow
    mov ax, es
    add ax, 0x1000
    mov es, ax
    xor bx, bx
.no_overflow:

    ; Dot every 20 sectors
    mov ax, [sectors_left]
    push ax
    mov cx, 20
    xor dx, dx
    div cx
    pop ax
    test dx, dx
    jnz .no_dot
    push ax
    mov al, '.'
    mov ah, 0x0E
    int 0x10
    pop ax
.no_dot:

    inc byte [cur_sector]
    cmp byte [cur_sector], 19
    jb .chs_loop
    mov byte [cur_sector], 1
    inc byte [cur_head]
    cmp byte [cur_head], 2
    jb .chs_loop
    mov byte [cur_head], 0
    inc byte [cur_cyl]
    jmp .chs_loop

.chs_error:
    mov [error_code], ah
    mov si, msg_disk_error
    call print_string
    mov al, [error_code]
    shr al, 4
    call .print_hex_digit
    mov al, [error_code]
    and al, 0x0F
    call .print_hex_digit
    mov si, msg_at_chs
    call print_string
    mov al, [cur_cyl]
    call .print_hex_byte
    mov al, ','
    mov ah, 0x0E
    int 0x10
    mov al, [cur_head]
    call .print_hex_byte
    mov al, ','
    mov ah, 0x0E
    int 0x10
    mov al, [cur_sector]
    call .print_hex_byte
    jmp halt16

.print_hex_byte:
    push ax
    shr al, 4
    call .print_hex_digit
    pop ax
    and al, 0x0F
.print_hex_digit:
    cmp al, 10
    jb .digit
    add al, 'A' - 10
    jmp .print_it
.digit:
    add al, '0'
.print_it:
    push ax
    mov ah, 0x0E
    int 0x10
    pop ax
    ret

; Disk Address Packet for LBA mode
align 4
dap:
    db 16           ; Size of DAP
    db 0            ; Reserved
dap_sectors:    dw 0    ; Sectors to read
dap_buffer_off: dw 0    ; Buffer offset
dap_buffer_seg: dw 0    ; Buffer segment
dap_lba_low:    dd 0    ; LBA low 32 bits
dap_lba_high:   dd 0    ; LBA high 32 bits

error_code: db 0
cur_sector: db 0
cur_head:   db 0
cur_cyl:    db 0
sectors_left: dw 0

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

    ; Copy kernel from 0x10000 to 0x100000
    mov esi, (KERNEL_LOAD_SEG << 4)         ; Source: 0x10000
    mov edi, KERNEL_ADDR                     ; Dest: 0x100000
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

    ; Map 16MB using 2MB huge pages (8 entries)
    ; This covers: kernel (1MB), heap (2-4MB), and leaves room for growth
    mov dword [PD_ADDR + 0],  0x00000083        ; 0-2MB
    mov dword [PD_ADDR + 8],  0x00200083        ; 2-4MB (heap)
    mov dword [PD_ADDR + 16], 0x00400083        ; 4-6MB
    mov dword [PD_ADDR + 24], 0x00600083        ; 6-8MB
    mov dword [PD_ADDR + 32], 0x00800083        ; 8-10MB
    mov dword [PD_ADDR + 40], 0x00A00083        ; 10-12MB
    mov dword [PD_ADDR + 48], 0x00C00083        ; 12-14MB
    mov dword [PD_ADDR + 56], 0x00E00083        ; 14-16MB

    ret

; ============================================================================
; 64-bit long mode code
; ============================================================================

[BITS 64]

long_mode:
    ; Enable SSE (required for 128-bit operations)
    ; Clear CR0.EM (bit 2) and set CR0.MP (bit 1)
    mov rax, cr0
    and ax, 0xFFFB          ; Clear EM
    or ax, 0x2              ; Set MP
    mov cr0, rax

    ; Set CR4.OSFXSR (bit 9) and CR4.OSXMMEXCPT (bit 10)
    mov rax, cr4
    or ax, (1 << 9) | (1 << 10)
    mov cr4, rax

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
vga_flag:       db 0                ; VGA debug flag: 0=disabled, 1=enable VGA mode 13h
                                    ; Offset from start of stage2: can be patched by Makefile
msg_stage2:     db "Stage 2: ", 0
msg_check_vga:  db "Checking VGA...", 0
msg_vga:        db "VGA mode 13h...", 0
msg_a20:        db "A20 line...", 0
msg_kernel:     db "Loading kernel...", 0
msg_ok:         db " OK", 13, 10, 0
msg_disk_error: db " DISK ERROR!", 13, 10, 0
msg_at_chs:     db " at CHS ", 0

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
