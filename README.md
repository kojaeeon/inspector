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
