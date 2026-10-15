# Process Garden

Windows·macOS용 Rust 네이티브 프로세스 모니터. 로컬에서만 동작하며 서버·AI 호출·자동 종료·자동실행 등록이 없다.

## 실행 및 설치

Rust 툴체인이 필요하며 최초 빌드에는 네트워크가 필요하다. 의존성은 Cargo.lock으로 고정한다.

```sh
cargo run --locked
```

### Windows

MSVC Rust 툴체인, Visual Studio C++ Build Tools 및 Windows SDK가 필요하다.

```powershell
$env:CARGO_BUILD_JOBS = '2'
cargo build --locked --release
.\scripts\install-windows.ps1
```

설치 경로는 `%LOCALAPPDATA%\Programs\Process Garden`이며 바탕 화면·시작 메뉴 바로가기를 만든다. 관리자 권한·자동실행 등록은 필요하지 않다. 자세한 설명은 [WINDOWS.md](WINDOWS.md)를 참고한다.

### macOS

Xcode Command Line Tools와 Rust 툴체인이 필요하다.

```sh
bash scripts/bundle.sh
open 'dist/Process Garden.app'
```

현재 빌드 머신 아키텍처용 앱을 생성하고 로컬 서명한다. Apple 공증은 하지 않으므로 다른 Mac에서 실행 시 Gatekeeper 확인이 필요할 수 있다.

## 기능

- 프로젝트 경로 및 역할(에이전트/로컬 LLM/MCP/개발 도구/브라우저/가상 기기) 분류
- 이름·PID·프로젝트 검색, 메모리·CPU·최신·오래된·중요도순 정렬
- 기본 전체 메모리순, 선택적으로 프로젝트별 그룹화
- 체크박스·Ctrl(Windows)/⌘(macOS) 클릭·Shift 범위 선택 및 확인 후 일괄 종료
- 최근 약 3분 RAM 추이와 검색 결과 CPU/메모리 상위 3개 차트
- 전체 RAM·사용 가능 RAM·스왑/페이지파일·프로세스 수, 3초 간격 백그라운드 갱신
- 보호/주의/일반 분류와 판단 근거. 일반은 종료 추천이 아니다.
- 부모·직접 자식 탐색, 실행 파일 확인
- PID·시작시각·실행 파일·소유권 재확인, 현재 앱과 조상·시스템·다른 사용자 보호

## 운영체제별 동작

| 항목 | Windows | macOS |
|---|---|---|
| 한글 글꼴 | 시스템 맑은 고딕 | 시스템 Apple SD Gothic Neo |
| 종료 | 확인 후 `taskkill /PID /F` 강제 종료 | 확인 후 `SIGTERM` 정상 종료 요청 |
| 시스템 보호 | Windows 경로 및 핵심 프로세스 | `/System/`, `/usr/libexec/` 및 핵심 프로세스 |
| 사용자 판별 | sysinfo 사용자 SID 비교 | sysinfo 사용자 UID 비교 |

종료 전 작업을 저장하고 앱 자체의 정상 종료를 우선한다. 종료 후 최대 2초 동안 결과를 확인한다. 일괄 종료는 각 대상을 순차 검증하므로 대상 수에 따라 수집 갱신이 지연될 수 있다. 프로세스 트리 전체 종료와 자동실행 해제는 제공하지 않는다.

## 범위와 제약

권한에 따라 실행 파일·작업 경로·메모리가 조회되지 않을 수 있다. 소유권이나 신원이 불명확한 프로세스는 종료하지 않는다. 프로젝트 분류는 cwd/명령 인자에서 Project, Projects, Repos, Documents/Codex 경로를 찾는 휴리스틱이다. 인자 전문은 화면이나 파일에 기록하지 않는다.

프로세스 메모리는 작업 집합/RSS이며 실제 회수 가능한 RAM을 뜻하지 않는다. 공유 페이지가 합계에 중복될 수 있다. CPU는 코어 합산이므로 100%를 넘을 수 있고 첫 CPU 표본은 다음 갱신부터 의미가 있다. 서비스는 종료 후 재시작될 수 있다.

종료 신원 확인과 PID 기반 종료 사이의 짧은 경쟁 구간은 남는다. 메모리 자동 청소기나 작업 필요 여부를 판정하는 AI는 아니다. 프로세스 정보는 외부로 전송하지 않는다.

## 검증

```sh
cargo fmt --check
cargo test --locked -- --test-threads=1
cargo build --locked --release
```

테스트는 실제 사용자 프로세스 대신 테스트가 생성한 자식만 종료한다. 소유권·신원·시스템 보호, Unicode 경로, 역할 분류, 선택/정렬, stale PID 거부 및 선택적 일괄 종료를 확인한다. 무시된 `child_sleep_helper`는 Windows 테스트 자식용 대기 함수다.

GitHub Actions에서 Windows와 macOS의 테스트·release 빌드를 실행하고, macOS 앱 번들도 검증한다. CI 빌드는 GUI 수동 검증을 대신하지 않는다. Windows GUI는 실제 Windows 11에서 확인했으며 macOS GUI의 현장 검증은 별도로 필요하다.

참고: [eframe](https://docs.rs/eframe/0.33.3/eframe/) · [sysinfo](https://docs.rs/sysinfo/0.37.2/sysinfo/)
