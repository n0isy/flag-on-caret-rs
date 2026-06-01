# flag-on-caret-rs

[![build-and-release](https://github.com/n0isy/flag-on-caret-rs/actions/workflows/release.yml/badge.svg)](https://github.com/n0isy/flag-on-caret-rs/actions/workflows/release.yml)
[![Release](https://img.shields.io/github/v/release/n0isy/flag-on-caret-rs?sort=semver)](https://github.com/n0isy/flag-on-caret-rs/releases/latest)
[![License: LGPL v3](https://img.shields.io/badge/License-LGPL_v3-blue.svg)](LICENSE)

**🇬🇧 [English documentation — README.md](README.md)**

Крошечная нативная Windows-утилита в трее, которая показывает **флажок текущей
раскладки клавиатуры**:

1. **рядом с текстовой кареткой**, и
2. **на курсоре мыши** — на текстовый I-курсор и на стрелку накладывается
   маленький флажок раскладки.

Без настроек. Релизный бинарь — **~300 КБ**, без рантайм-зависимостей.

Это **переписанная на Rust** версия проекта
[`n0isy/flag-on-caret`](https://github.com/n0isy/flag-on-caret) (вырезанная одна
функция из [**LangBarXX**](https://github.com/Krot66/LangBarXX) авторства
**Krot66**). Поведение, картинки флажков/курсоров и значения по умолчанию взяты
оттуда — сменился только язык реализации. См. [CREDITS.md](CREDITS.md).

---

## Статус — полный паритет с оригиналом на AHK

| Часть | Состояние |
|------|-------|
| Иконка в трее + меню **Выход** | ✅ (`trayicon`) |
| Раскладка активного окна | ✅ (`GetKeyboardLayout`) |
| Флаг у каретки — классические Win32-контролы | ✅ (`GetGUIThreadInfo`) |
| Флаг у каретки — **браузеры на Chromium** | ✅ MSAA `OBJID_CARET` + `accLocation`; скрывается при потере фокуса через `accState` |
| Флаг у каретки — **UWP / новый Notepad** | ✅ UIA `TextPattern2.GetCaretRange`; скрывается при потере фокуса через `isActive` |
| Флаг на **собственных курсорах пользователя** | ✅ настоящие стрелка/I-курсор захватываются при старте (`DrawIconEx` по чёрному+белому → истинная альфа) + hotspot, флаг накладывается, `SetSystemCursor` (восстановление через `SPI_SETCURSORS` при выходе) |
| **Контраст I-курсора** (бел./чёрн. по фону) | ✅ выборка `GetPixel` + инвертирующая матрица GDI+ (статичный курсор не может XOR-ить пиксели, как Windows) |
| **Консольная раскладка** (Win+Space в conhost) | ✅ `AttachConsole` + `GetConsoleKeyboardLayoutNameW`, кэш по окну; не-conhost терминалы (far2l) детектятся, и флаг подавляется (см. ниже) |
| PNG-флаг по локали + **текстовый фоллбэк** | ✅ полная таблица `LangCode` из LangBarXX (287) + градиентный текстовый флаг на GDI+ |
| Защиты: полный экран, **меню #32768**, **secure desktop** | ✅ |
| **Единственный экземпляр**, per-monitor-v2 DPI, сброс курсоров перед захватом | ✅ |

Определение каретки (`src/caret.rs`) — точный порт `GetCaretLocation.ahk` из
LangBarXX: диспетчеризация по классу окна UIA → MSAA → `GetGUIThreadInfo` с тем
же провалом по цепочке.

### Известные ограничения
- **far2l** (и другие не-conhost терминалы): раскладку **нельзя прочитать извне
  процесса** — far2l переключается через TSF собственного потока и не трогает ни
  legacy-HKL, ни консольное имя раскладки, ни какой-либо межпоточно-читаемый API
  (мы проверили все, включая WinRT `CurrentInputMethodLanguageTag`, который по
  документации работает только на потоке с фокусом ввода). Чтобы не рисовать
  *неверный* флаг, приложение **детектит этот случай и не показывает флаг** там —
  консольное окно, у которого `GetConsoleKeyboardLayoutNameW` не отвечает,
  считается ненадёжным, и флаг каретки и курсора над ним скрывается. conhost-овые
  терминалы (cmd, mingw) и mintty работают нормально.
- Флаг курсора вкомпонован в **статичный** системный курсор, поэтому I-курсор
  выбирает один контрастный цвет по фону, а не инвертирует попиксельно.

> Программа подменяет **системные** курсоры I-beam/стрелки на время работы и
> восстанавливает их при чистом выходе (а также сбрасывает их перед захватом,
> чтобы упавший прошлый запуск не «отравил» следующий). Если процесс убит
> жёстко — восстановить можно через **Панель управления → Мышь → OK**.

## Почему здесь Rust

Мы измеряли компромисс против AHK-версии (см. обсуждение в соседнем репозитории):
по-настоящему сложная часть — определение каретки в разных типах приложений —
одинаковой сложности на любом языке, но нативная сборка убирает рантайм AHK и
ужимает бинарь до ~300 КБ. `trayicon` закрывает потребность в «простейшем трее
под Windows» чисто Win32-путём; всё остальное — `windows-sys`.

## Сборка

Нативно (рекомендуется), на Windows с тулчейном Rust:

```bash
cargo build --release      # -> target/release/FlagOnCaret.exe
```

Кросс-компиляция из Linux (то, чем пользуются локальные проверки уровня CI):

```bash
rustup target add x86_64-pc-windows-gnu
sudo apt-get install -y gcc-mingw-w64-x86-64
cargo build --release --target x86_64-pc-windows-gnu
```

`FlagOnCaret.exe` **самодостаточен** — PNG-флаги и заготовки курсоров вшиты в
бинарь через `include_bytes!` и декодируются из памяти средствами GDI+
(`SHCreateMemStream` + `GdipCreateBitmapFromStream`), внешних файлов для поставки
нет.

Каждый релиз даёт две загрузки:

| Файл | Что это |
|------|------------|
| `FlagOnCaret_setup.exe` | Инсталлятор Inno Setup (ярлыки, опциональный автозапуск, деинсталляция). |
| `FlagOnCaret_portable.zip` | Просто отдельный `FlagOnCaret.exe`. |

Инсталлятор собирается из [`installer/FlagOnCaret.iss`](installer/FlagOnCaret.iss)
через Inno Setup 6 (`ISCC`); CI собирает его на каждом теговом релизе.

## Зависимости (самые свежие)

| Крейт | Версия | Зачем |
|-------|---------|-----|
| [`windows-sys`](https://crates.io/crates/windows-sys) | 0.61 | сырой Win32 + GDI+ FFI (окно, курсор, GDI+) |
| [`windows`](https://crates.io/crates/windows) | 0.62 | типизированный COM для UI Automation + MSAA-каретки |
| [`trayicon`](https://crates.io/crates/trayicon) | 0.4 | иконка в трее + меню (Windows-путь = только `winapi`) |

Rust edition **2024**.

## Лицензия

**LGPL-3.0** — как у LangBarXX. Условия сторонних библиотек см. в
[CREDITS.md](CREDITS.md).
