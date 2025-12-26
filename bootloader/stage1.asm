; Ralph OS Stage 1 Bootloader
; This is the boot sector (512 bytes) loaded by BIOS at 0x7C00
; It loads stage 2 and jumps to it

[BITS 16]
[ORG 0x7C00]

STAGE2_ADDR     equ 0x7E00      ; Where to load stage 2
STAGE2_SECTORS  equ 16          ; Number of sectors for stage 2 (8KB)

start:
    ; Set up segments
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00              ; Stack grows down from boot sector

    ; Save boot drive number
    mov [boot_drive], dl

    ; Print loading message
    mov si, msg_loading
    call print_string

    ; Load stage 2 from disk
    mov ah, 0x02                ; BIOS read sectors function
    mov al, STAGE2_SECTORS      ; Number of sectors to read
    mov ch, 0                   ; Cylinder 0
    mov cl, 2                   ; Start from sector 2 (sector 1 is boot sector)
    mov dh, 0                   ; Head 0
    mov dl, [boot_drive]        ; Drive number
    mov bx, STAGE2_ADDR         ; Destination address ES:BX
    int 0x13
    jc disk_error               ; Jump if carry flag set (error)

    ; Verify we read the expected number of sectors
    cmp al, STAGE2_SECTORS
    jne disk_error

    ; Print success message
    mov si, msg_ok
    call print_string

    ; Jump to stage 2, passing boot drive in DL
    mov dl, [boot_drive]
    jmp STAGE2_ADDR

disk_error:
    mov si, msg_disk_error
    call print_string
    jmp halt

halt:
    cli
    hlt
    jmp halt

; Print null-terminated string at SI
print_string:
    push ax
    push bx
    mov ah, 0x0E                ; BIOS teletype function
    mov bh, 0                   ; Page 0
.loop:
    lodsb                       ; Load byte from SI into AL
    test al, al                 ; Check for null terminator
    jz .done
    int 0x10                    ; Print character
    jmp .loop
.done:
    pop bx
    pop ax
    ret

; Data
boot_drive:     db 0
msg_loading:    db "Ralph OS: Loading stage 2...", 0
msg_ok:         db " OK", 13, 10, 0
msg_disk_error: db " DISK ERROR!", 13, 10, 0

; Padding and boot signature
times 510 - ($ - $$) db 0
dw 0xAA55
