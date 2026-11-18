# Office DRM Inspector

Excel, Word, PowerPoint에서 문서를 열 때 인증창이 반복되는 상황을 진단하는 Windows x64 CLI입니다. 대화상자를 띄운 프로세스와 소유 창의 프로세스, 열림·닫힘 시점과 재표시 간격을 확인할 수 있습니다.

## 실행

일반 사용자 권한으로 Office와 같은 Windows 데스크톱 세션에서 PowerShell을 엽니다. 수집을 시작한 다음 평소대로 문서를 열고 증상을 재현합니다.

```powershell
.\inspector.exe watch --duration 120 --out .\office-auth.jsonl
.\inspector.exe summarize .\office-auth.jsonl
```

`watch`는 지정한 시간 뒤 또는 Ctrl+C로 종료합니다. 기본값은 120초와 `office-auth.jsonl`이며, 기존 로그 파일을 보존하므로 새 파일명을 사용하면 재수집할 수 있습니다. `summarize`는 한 수집 세션의 로그에서 타임라인과 그룹별 집계를 출력합니다. JSONL의 `utc_ms`는 Unix epoch 기준 UTC 밀리초, `elapsed_ms`는 수집 시작 이후 경과 시간입니다.

| 표시 | 의미 |
|---|---|
| `Office process` | 창을 생성한 프로세스가 Excel, Word 또는 PowerPoint입니다. |
| `Office owner` | Windows가 보고한 소유 창의 프로세스가 Office입니다. |
| `time proximity` | Office가 활성 창이던 동안, 또는 Office에서 포커스가 이탈한 뒤 15초 이내에 관찰했습니다. 다른 앱의 창도 해당할 수 있습니다. |
| `foreground candidate` | 활성화만 관찰한 외부 후보입니다. 대화상자 표시 횟수에 포함되지 않습니다. |

대화상자 시작·종료 이벤트와 표준 Win32 대화상자 클래스(`#32770`)의 표시 이벤트를 관찰합니다. 한 그룹(프로세스명, PID, 창 클래스)의 열린 대화상자가 모두 닫힌 후 같은 그룹의 창이 열리면 재표시로 집계합니다. 같은 클래스의 서로 다른 대화상자가 한 그룹에 포함될 수 있습니다. 프로그램이 해당 이벤트를 보내지 않거나 창이 다른 데스크톱에서 실행되면 탐지가 제한됩니다. 기록은 인증이 재요청된 원인이나 DRM 제품을 직접 판정할 수 없습니다.

로그에는 이벤트 시각, 세션 내부 창 ID, PID, 실행 파일명, 창 클래스와 소유 프로세스 정보만 저장합니다. 창 제목·본문, 입력값, 문서 내용, 서버 주소, 실행 파일 전체 경로는 읽지 않습니다. 결과는 지정한 로컬 파일에 기록됩니다.

## 빌드 및 검증

Windows x64의 Rust stable과 MSVC 빌드 도구를 사용합니다.

```powershell
$env:RUSTFLAGS = '-C target-feature=+crt-static'
cargo test --locked --target x86_64-pc-windows-msvc
cargo build --locked --release --target x86_64-pc-windows-msvc
.\tests\windows-smoke.ps1 -Inspector .\target\x86_64-pc-windows-msvc\release\inspector.exe
```

스모크 테스트는 Office 없이 가상 `WINWORD.exe` 프로세스에서 표준 대화상자를 두 번 표시하고 한 번의 재표시가 집계되는지 확인합니다. Windows GitHub Actions에서 빌드된 실행 파일은 실행 결과의 `inspector-windows-x64` 아티팩트로 제공됩니다.

## DLL 추적 (선택)

```powershell
# Office와 관련 창의 프로세스 및 소유 프로세스를 자동 추적
.\inspector.exe watch --dll --duration 120 --out .\office-dll.jsonl
.\inspector.exe summarize .\office-dll.jsonl

# 인증창 이벤트가 잡히지 않으면 작업 관리자에서 확인한 PID 직접 지정
# --pid는 반복 가능하며 --dll을 자동으로 켭니다.
.\inspector.exe watch --pid 12676 --pid 17056 --duration 120 --out .\office-pids.jsonl
```

`--dll`을 켜면 Win32 ToolHelp 스냅샷으로 대상 프로세스의 DLL 전체 경로를 기록합니다.
첫 성공 스캔은 `module_baseline`(이미 로드된 DLL), 이후 500ms 간격 비교는
`module_added`/`module_removed`(관찰된 추가/제거), 프로세스 종료는 `module_process_exited`입니다.
각 기록에 UTC/경과 시간, PID, 프로세스명, DLL 경로가 있으므로 인증창 시각과 비교할 수 있습니다.
창 제목/내용, 키 입력, 비밀번호, 문서 본문, 프로세스 메모리는 읽지 않습니다.
DLL 경로에는 사용자명과 설치 경로가 포함될 수 있습니다. 로그 공유 전 확인하세요.

- 먼저 수집을 시작하고 문서를 여세요. 이미 로드된 DLL은 로드 시각을 알 수 없습니다.
- 샘플링 사이에 로드/언로드된 짧은 DLL, 수동 매핑, 보호 프로세스는 누락될 수 있습니다.
- DLL이 동시에 관찰됐다는 사실만으로 인증창 원인이나 DRM 제품으로 확정하지 않습니다.
- 일반 사용자 권한으로 시작하세요. 접근 불가 시 `module_scan_error`와 Win32 오류 번호를 남기며 창 수집은 계속됩니다.
  PID 직접 지정이 실패하면 권한/프로세스 상태를 확인하고 새 수집을 시작하세요.
- 스캔 실패를 DLL 제거로 처리하지 않으며 같은 오류는 반복 출력하지 않습니다.
- 대상 프로세스 핸들을 유지하여 PID 재사용을 다른 프로세스로 이어서 추적하지 않습니다.
  한 세션에서 최대 32개 PID를 시도합니다. 초과 PID는 `module_target_limit`으로 표시합니다.
- 프로세스 주입/디버거 부착/보안 설정 변경 없이 모듈 목록만 읽습니다. 기본 watch는 기존처럼 DLL 수집을 하지 않습니다.
- 기존 창 전용 JSONL도 새 summarize에서 그대로 읽을 수 있습니다.

Windows CI는 가상 WINWORD.exe에서 테스트 DLL을 실제 로드/해제하여 baseline·추가·제거 이벤트와
기존 인증창 2회/재표시 집계를 함께 검사합니다. 회사 Office/DRM 환경은 별도 재현이 필요합니다.

## Optional credential helper (manual confirmation)

This is a separate executable, not an unattended keylogger or authentication bypass.
It reads `OFFICE_AUTH_USER` and `OFFICE_AUTH_PASSWORD` from the current process environment, requires typing `YES`, then waits 5 seconds for the operator to focus the intended Windows Security dialog with an **empty username field**. It refuses to type unless the foreground window title contains `Windows Security` or `Windows 보안`. It types username, Tab, password, Enter. It does **not** verify the target server or authentication success, so use only with an explicitly confirmed intended dialog and an approved test account. Windows may block synthetic input to protected credential dialogs. It does not store or print credentials.

```powershell
$env:OFFICE_AUTH_USER = "approved-user"
$env:OFFICE_AUTH_PASSWORD = Read-Host "Password"
cargo build --release --bin auth_helper
.\target\release\auth_helper.exe
Remove-Item Env:OFFICE_AUTH_PASSWORD
```

Avoid persistent machine/user environment variables for passwords. For production, prefer Windows Credential Manager or a supported SSO fix. This helper is not a substitute for the server-side authentication repair.

## Continuous auth helper (experimental)

`auth_helper.exe` continuously checks whether Excel, Word, or PowerPoint is running and whether the foreground window title contains both `Windows Security` (or `Windows 보안`) and the configured `OFFICE_AUTH_TARGET`. Only then does it type `OFFICE_AUTH_USER`, Tab, and `OFFICE_AUTH_PASSWORD`; it never presses Enter or clicks Confirm. It needs the username field already focused. Passwords are not logged.

**Important limitation:** Many Windows Security / XAML credential prompts display the server only in their body, not in the top-level window title. In those cases this helper intentionally does nothing. It does not yet inspect UI Automation child elements or prove server identity, and therefore is not a verified solution for the reported corporate prompt. Test only with an approved account and dialog. Synthetic input may be blocked by Windows security boundaries.

```powershell
$env:OFFICE_AUTH_USER = 'approved-user'
$env:OFFICE_AUTH_PASSWORD = Read-Host 'Password'
$env:OFFICE_AUTH_TARGET = 'scadmin.standardchartered.co.kr'
.\auth_helper.exe
```

The startup installer currently launches the **inspector event monitor only**, not the credential helper. Do not configure unattended credential input until the actual dialog can be identified reliably.

### UI Automation credential helper (Windows PowerShell 5.1)

The portable archive includes `office-auth-uia.ps1`. This is the **UI-element inspection** implementation; it is separate from the older Rust `auth_helper.exe` which only checks window titles.

```powershell
$env:OFFICE_AUTH_USER = 'YOUR_ACCOUNT'
$env:OFFICE_AUTH_PASSWORD = Read-Host 'Office authentication password'
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\office-auth-uia.ps1
```

The helper runs until Ctrl+C. It checks for a running Office application, then inspects the foreground Windows Security dialog's descendant UI Automation Text/Hyperlink names. It requires an exact `scadmin.standardchartered.co.kr` hostname token, a non-password Edit control, and an `IsPassword` Edit control with writable ValuePattern. It fills those controls once per foreground window, without pressing Enter or clicking Confirm. **If Windows does not expose the hostname or the password ValuePattern, no automatic input occurs.** This does not establish cryptographic server identity. Do not persist the password in system-wide environment variables.

This script has not been tested against the real corporate authentication dialog. The existing startup installer launches the event monitor only, not this script.
