# BlueOS

BlueOS — маленькая учебная ОС, написанная **с нуля на Rust и ассемблере**, без Linux, `std`, libc, GRUB и готового загрузчика. Один репозиторий собирает два bare-metal ядра:

| Платформа | Загрузка | Графика | Сеть | Ввод |
|---|---|---|---|---|
| x86_64 UEFI | нативное Rust UEFI-приложение `BOOTX64.EFI` | GOP 32-bit framebuffer | firmware SNP + ARP/IPv4/ICMP/UDP/DNS | Simple Text Input + Simple Pointer |
| x86_64 Legacy BIOS | собственные stage1/stage2 на GNU Assembly, long mode | VESA VBE 0x118, linear framebuffer 1024×768×24 | Intel e1000 + PCI | PS/2-клавиатура и мышь |
| RISC-V 64 `virt` | OpenSBI + точка входа на RISC-V Assembly | virtio-gpu, framebuffer 800×600×32 | virtio-net MMIO | UART/serial |

В ядро также встроены:

- собственный Plasma-подобный рабочий стол: обои, панель, launcher, переключение приложений, перемещаемые мышью окна, часы, терминал, файлы, настройки и установщик (это не код KDE/Qt);
- UEFI GUI-установщик с обязательным выбором диска, фразой подтверждения `INSTALL ERASE`, прогрессом, flush и полным read-back сравнением;
- read-only UEFI-файловый менеджер, который перечисляет корни доступных firmware filesystem volumes без риска записи;
- allocation-free компилятор Lua-подобного подмножества в байткод и стековая VM;
- Ethernet, ARP, IPv4, ICMP echo, UDP и DNS-клиент через e1000/virtio или firmware UEFI SNP;
- интерактивная командная строка (`HELP`, `INFO`, `CLEAR`, `LUA`, `NET`, `DNS`, `FILES`, `SETTINGS`, `INSTALL`);
- serial-лог для диагностики обеих архитектур.

> Это самостоятельное минимальное ядро/MVP, а не замена Linux. Lua-компилятор поддерживает полезное встроенное подмножество (`local`, числа, строки, арифметику, переменные, `print`, `return`), но не заявляет совместимость со всем Lua 5.x. GOP/VBE позволяют загрузить графику на многих AMD64 ПК, однако native xHCI/AHCI/NVMe, Wi-Fi, Radeon acceleration, TLS/TCP и универсальные драйверы реального оборудования ещё не готовы. Установщик работает через UEFI Block I/O и намеренно отказывается писать, если не может отличить съёмный источник от целого внутреннего диска. Честный статус каждого аппаратного класса приведён в [матрице совместимости AMD64](docs/amd64-hardware.md).

## Быстрый старт

### 1. Инструменты

Нужны GNU `as`, `ld`, `objcopy`, Rust stable и bare-metal targets. Скрипт установит Rust без root:

```bash
./scripts/install-toolchain.sh
```

Для запуска установите QEMU (Debian/Ubuntu):

```bash
sudo apt install qemu-system-x86 qemu-system-misc ovmf
```

### 2. Сборка обоих образов

```bash
make all
```

Результат:

```text
images/blueos-x86_64.img    hybrid raw image: Legacy BIOS + FAT16 ESP для UEFI
images/blueos-BOOTX64.EFI   отдельное UEFI-приложение для диагностики
images/blueos-x86_64.elf    ELF Legacy BIOS-ядра с символами для GDB
images/blueos-riscv64.elf   загружаемый OpenSBI/QEMU RISC-V image
```

Структурная проверка обоих файлов:

```bash
make check
```

### Сборка в GitHub Actions

Готовый workflow хранится в [`ci/github-actions-build.yml`](ci/github-actions-build.yml). Он собирает обе архитектуры, запускает Legacy BIOS, OVMF/UEFI и RISC-V в QEMU, проверяет serial/debug-логи и публикует образы как artifact `blueos-qemu-images`.

Чтобы включить или обновить CI, владелец репозитория должен через GitHub скопировать этот шаблон в `.github/workflows/build.yml`. Шаблон нельзя автоматически положить туда через ограниченное подключение GitHub App без разрешения `workflows`; после изменений `ci/github-actions-build.yml` установленный workflow тоже нужно синхронизировать вручную.

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

### 4. Запуск x86_64 UEFI

После установки OVMF тот же hybrid-образ загружается как UEFI-диск:

```bash
make run-uefi
```

Скрипт ищет стандартные `OVMF_CODE*.fd`/`OVMF_VARS*.fd`; нестандартные пути можно передать через `OVMF_CODE` и `OVMF_VARS`. Горячие клавиши UEFI workspace: `F1` launcher, `F2` terminal, `F3` files, `F4` settings, `F8` installer.

Для физического компьютера запишите **весь** `images/blueos-x86_64.img` на USB через Rufus в DD mode или аналогичный raw-image writer. Отключать Secure Boot обязательно: BlueOS EFI-файл пока не подписан. Рекомендуется сначала отключить все диски с ценными данными и проверить live desktop без запуска установки.

Установщик никогда не начинает запись автоматически:

1. `INSTALL` или `F8` сначала проверяет MBR/ESP layout съёмного источника и показывает только целые writable non-removable UEFI Block I/O диски;
2. `INSTALL SELECT N` явно выбирает один из показанных дисков;
3. `INSTALL ERASE` — отдельная разрушительная фраза подтверждения;
4. после записи выполняются firmware flush и полное поблочное сравнение первых 64 MiB источника и цели.

Если removable live USB или безопасная цель не распознаны, установщик остаётся read-only. Текущий прототип копирует 64-MiB hybrid image и не расширяет раздел на оставшееся место диска.

### 5. Запуск RISC-V 64

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

- `DESKTOP`, `FILES`, `SETTINGS`, `INSTALL` — переключить графический workspace;
- `LUA` — повторно скомпилировать исходник и запустить байткод;
- `NET` — показать IP, ARP-состояние и счётчики пакетов;
- `DNS` — разрешить `example.com` через виртуальный DNS QEMU;
- `INFO` — сведения о ядре;
- `CLEAR` — очистить framebuffer-терминал.

Legacy x86_64 и RISC-V используют стандартную адресацию QEMU user networking: гостевой IP `10.0.2.15`, gateway `10.0.2.2`, DNS `10.0.2.3`. UEFI workspace через firmware SNP выполняет allocation-free DHCP Discover/Request, принимает адрес gateway/DNS из ACK, затем запускает ARP и DNS. Команда `NET` повторяет незавершённый DHCP/ARP-запрос, а `DNS` разрешает `example.com` через выданный сервер. ICMP echo request, пришедший на текущий адрес, получает ответ. DHCP lease renewal и ручная configuration UI ещё не реализованы.

## Как устроено

```text
boot/x86/                  BIOS sector, VBE loader, protected/long mode
linker/                    linker scripts для физической раскладки ядер
src/bin/blueos_uefi.rs     UEFI live workspace и guarded Block I/O installer
src/uefi.rs                dependency-free UEFI ABI и GUID протоколов
src/arch/uefi.rs           GOP/console firmware adapter
src/arch/x86_64.rs         ports, serial, PS/2, PCI и e1000
src/arch/riscv64.rs        RISC-V entry/runtime и UART
src/arch/riscv64/virtio.rs virtio MMIO queues, GPU и network
src/framebuffer.rs         BGR/RGB renderer и custom Plasma-like UI
src/lua.rs                 lexer, compiler, bytecode и VM без heap
src/net.rs                 ARP/IPv4/ICMP/UDP/DNS
scripts/                    toolchain, hybrid FAT image builder и QEMU
```

### x86_64 boot flow

1. 512-байтный stage1 читает stage2 через BIOS INT 13h extensions.
2. Stage2 включает A20, задаёт VESA VBE linear framebuffer и загружает Rust kernel.
3. Ассемблер создаёт GDT и 4-уровневые page tables, identity-map первых 4 GiB huge pages.
4. Процессор переключается в long mode и передаёт `BootInfo` в Rust `_start`.

### x86_64 UEFI boot flow

Hybrid MBR сохраняет BIOS-код в секторе 0 и одновременно описывает ESP type `0xEF`, начинающийся с LBA 2048. В FAT16-разделе лежит стандартный fallback path `EFI/BOOT/BOOTX64.EFI`. Rust UEFI-приложение выбирает доступный GOP mode, учитывает RGB/BGR layout, оставляет Boot Services активными для keyboard и Block I/O и запускает тот же framebuffer workspace. Secure Boot пока не поддержан.

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
