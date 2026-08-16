# BlueOS

BlueOS — маленькая учебная ОС, написанная **с нуля на Rust и ассемблере**, без Linux, `std`, libc, GRUB и готового загрузчика. Один репозиторий собирает два bare-metal ядра:

| Платформа | Загрузка | Графика | Сеть | Ввод |
|---|---|---|---|---|
| x86_64 BIOS | собственные stage1/stage2 на GNU Assembly, long mode | VESA VBE 0x118, linear framebuffer 1024×768×32 | Intel e1000 + PCI | PS/2-клавиатура |
| RISC-V 64 `virt` | OpenSBI + точка входа на RISC-V Assembly | virtio-gpu, framebuffer 800×600×32 | virtio-net MMIO | UART/serial |

В ядро также встроены:

- оконно-подобный графический экран и framebuffer-терминал;
- allocation-free компилятор Lua-подобного подмножества в байткод и стековая VM;
- Ethernet, ARP, IPv4, ICMP echo, UDP и DNS-клиент;
- интерактивная командная строка (`HELP`, `INFO`, `CLEAR`, `LUA`, `NET`, `DNS`);
- serial-лог для диагностики обеих архитектур.

> Это самостоятельное минимальное ядро/MVP, а не замена Linux. Lua-компилятор поддерживает полезное встроенное подмножество (`local`, числа, строки, арифметику, переменные, `print`, `return`), но не заявляет совместимость со всем Lua 5.x. Сетевые драйверы рассчитаны на эмулируемые e1000/virtio устройства QEMU; USB, Wi-Fi, TLS, TCP и драйверы произвольного реального оборудования пока отсутствуют.

## Быстрый старт

### 1. Инструменты

Нужны GNU `as`, `ld`, `objcopy`, Rust stable и bare-metal targets. Скрипт установит Rust без root:

```bash
./scripts/install-toolchain.sh
```

Для запуска установите QEMU (Debian/Ubuntu):

```bash
sudo apt install qemu-system-x86 qemu-system-misc
```

### 2. Сборка обоих образов

```bash
make all
```

Результат:

```text
images/blueos-x86_64.img   загрузочный raw BIOS disk image
images/blueos-x86_64.elf   ELF с символами для GDB
images/blueos-riscv64.elf  загружаемый OpenSBI/QEMU RISC-V image
```

Структурная проверка обоих файлов:

```bash
make check
```

### 3. Запуск x86_64

```bash
./scripts/run-x86_64.sh
```

Эквивалентная полная команда:

```bash
qemu-system-x86_64 \
  -machine pc -m 128M \
  -drive format=raw,file=images/blueos-x86_64.img,if=ide,index=0 \
  -device VGA \
  -netdev user,id=blueosnet \
  -device e1000,netdev=blueosnet,mac=52:54:00:12:34:56 \
  -serial stdio -no-reboot
```

Команды вводятся в графическом окне QEMU через PS/2-клавиатуру. Serial-лог остаётся в терминале.

### 4. Запуск RISC-V 64

```bash
./scripts/run-riscv64.sh
```

Эквивалентная команда:

```bash
qemu-system-riscv64 \
  -machine virt -m 256M -smp 1 \
  -bios default -kernel images/blueos-riscv64.elf \
  -device virtio-gpu-device \
  -netdev user,id=blueosnet \
  -device virtio-net-device,netdev=blueosnet,mac=52:54:00:12:34:57 \
  -serial mon:stdio -no-reboot
```

На RISC-V команды вводятся в serial-консоли. Для переключения из QEMU monitor в serial-консоль при необходимости нажмите `Ctrl-A`, затем `c`.

## Проверка функций

После загрузки ядро само компилирует и исполняет встроенную программу:

```lua
local answer = 6 * 7
local blue = answer + 1
print("Lua bytecode VM online")
print(blue)
return 0
```

Полезные команды:

- `LUA` — повторно скомпилировать исходник и запустить байткод;
- `NET` — показать IP, ARP-состояние и счётчики пакетов;
- `DNS` — разрешить `example.com` через виртуальный DNS QEMU;
- `INFO` — сведения о ядре;
- `CLEAR` — очистить framebuffer-терминал.

Сеть использует стандартную адресацию QEMU user networking: гостевой IP `10.0.2.15`, gateway `10.0.2.2`, DNS `10.0.2.3`. После ARP драйвер автоматически отправляет DNS-запрос. ICMP echo request, пришедший на `10.0.2.15`, получает ответ.

## Как устроено

```text
boot/x86/                 BIOS sector, VBE loader, protected/long mode
linker/                   linker scripts для физической раскладки ядер
src/arch/x86_64.rs        ports, serial, PS/2, PCI и e1000
src/arch/riscv64.rs       RISC-V entry/runtime и UART
src/arch/riscv64/virtio.rs virtio MMIO queues, GPU и network
src/framebuffer.rs        32-bit BGRX renderer и UI
src/lua.rs                lexer, compiler, bytecode и VM без heap
src/net.rs                ARP/IPv4/ICMP/UDP/DNS
scripts/                   установка toolchain и команды QEMU
```

### x86_64 boot flow

1. 512-байтный stage1 читает stage2 через BIOS INT 13h extensions.
2. Stage2 включает A20, задаёт VESA VBE linear framebuffer и загружает Rust kernel.
3. Ассемблер создаёт GDT и 4-уровневые page tables, identity-map первых 4 GiB huge pages.
4. Процессор переключается в long mode и передаёт `BootInfo` в Rust `_start`.

### RISC-V boot flow

OpenSBI передаёт управление `_start` по адресу `0x80200000`. Ассемблер очищает BSS, создаёт стек и вызывает Rust. Драйвер сканирует восемь QEMU virtio-MMIO slots, согласует `VIRTIO_F_VERSION_1`, настраивает split virtqueues и создаёт GPU resource/scanout.

## Отладка

x86_64 с GDB stub:

```bash
qemu-system-x86_64 -S -s -drive format=raw,file=images/blueos-x86_64.img \
  -device VGA -serial stdio
# другой терминал:
gdb images/blueos-x86_64.elf -ex 'target remote :1234'
```

Очистить промежуточные файлы:

```bash
make clean
```

Исходный код распространяется по MPL-2.0.
