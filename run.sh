set -e

cargo build --target x86_64-unknown-uefi
mkdir -p esp/efi/boot
cp target/x86_64-unknown-uefi/debug/unikernel.efi esp/efi/boot/bootx64.efi
qemu-system-x86_64 -enable-kvm \
    -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd \
    -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_VARS.fd \
    -drive format=raw,file=fat:rw:esp\
    -serial stdio\
    #-display none