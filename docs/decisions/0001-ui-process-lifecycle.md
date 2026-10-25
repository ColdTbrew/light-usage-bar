# ADR 0001: 메뉴 막대 host와 GPUI 패널의 프로세스 수명

- 상태: **제안 / 검증 대기**
- 날짜: 2026-10-25
- 프로젝트: Light Usage Bar
- 대상 플랫폼: macOS 전용
- 결정 책임: prototype 구현 및 benchmark 결과를 바탕으로 확정

## 배경

사용자는 Claude, Codex, GrokBot(xAI 구독), Antigravity 사용량을 표시하는 앱을 항상 실행하면서 자원 부담을 최소화하기 원한다. Rust와 GPUI를 사용하되, 패널을 열지 않는 시간이 대부분인 메뉴 막대 앱에 맞게 설계해야 한다. 다른 사용량 앱과 RAM을 같은 조건에서 비교하는 것도 제품 목표다.

사용자의 **저자원 상주 앱이라는 우선순위와 macOS 전용 범위는 확정**이다. GPUI를 별도 프로세스로 실행하는 세부 방식은 아직 측정하지 않았으므로 확정된 성능 최적화로 취급하지 않는다.

## 제안

상주하는 Rust host가 네이티브 AppKit `NSStatusItem`, provider probe, 공통 snapshot cache를 소유한다. 상세 패널은 사용자가 요청할 때만 GPUI로 표시한다.

**우선 검증 후보는 짧게 실행하는 GPUI helper**다. Host에는 GPUI를 링크하지 않는 구성을 만들고, 클릭 시 app bundle 내부 helper를 하나 실행한다. Helper는 표시용 snapshot만 받아 렌더링하고 닫히면 종료한다. 자격 증명과 provider 조회는 host/probe에 둔다.

동일 기능의 **이벤트 기반 단일 프로세스 후보도 구현·측정**한다. 단일 프로세스에서는 처음에 GPUI 창을 열지 않고 클릭 때 생성하며, 닫을 때 `remove_window()`와 view/task cleanup을 수행한다. 두 후보를 비교하기 전 이 ADR을 `채택`으로 변경하지 않는다.

## 비교

| 항목 | Rust host + 단명 GPUI helper | 이벤트 기반 단일 프로세스 |
| --- | --- | --- |
| 닫힌 상태 | GPUI 프로세스가 존재하지 않도록 수명 경계를 명확히 설정 가능 | 창이 없더라도 process/platform/cache 상태가 남을 수 있음 |
| 첫 클릭 | 프로세스·GPUI·Metal 초기화 비용 발생 | 기존 application context를 재사용 가능 |
| 자주 여닫기 | 반복 시작·종료 비용을 측정해야 함 | 생성·제거할 창과 재사용 cache의 균형을 측정해야 함 |
| IPC | schema, timeout, helper singleton 관리 필요 | 내부 state 전달로 단순화 |
| 실패 격리 | helper crash 후에도 host가 유지될 수 있음 | UI crash가 상주 앱 전체에 영향을 줄 수 있음 |
| 배포 | 하나의 `.app` 내부 helper도 서명·공증 범위에 포함 | executable 구조가 상대적으로 단순 |
| 메모리 판정 | host·helper·probe를 함께 측정해야 함 | 단일 PID만 보는 경우에도 CLI 자식은 포함 |
| 현재 우열 | **미측정** | **미측정** |

프로세스를 분리하면 닫힌 UI의 실행 및 메모리 소유권을 끊는 경계가 분명해진다. 다만 그 사실이 모든 상황에서 더 낮은 총 RAM·전력을 보장하지는 않는다. 반복 초기화, 공유 라이브러리 accounting, allocator와 driver cache, panel 사용 빈도에 따라 결과가 달라질 수 있다.

## 두 후보의 공통 조건

- 메뉴 막대는 Rust 바인딩을 통한 AppKit `NSStatusItem`으로 구현한다.
- 100~150ms 단위 tray event polling, 상시 animation, 초 단위 quota 조회를 두지 않는다.
- 바깥 클릭 monitor와 UI countdown은 패널이 열린 동안만 유지한다.
- 기본 배경 fetch는 10분이며, 하나의 scheduler와 tolerance를 사용한다.
- 패널은 cache를 먼저 표시한다. stale 조회는 host에 요청하며 `single-flight`, minimum interval, backoff를 유지한다.
- provider CLI나 브라우저를 감시용으로 상주시키지 않는다. 필요한 probe는 deadline을 가진 작업으로 제한한다.
- IPC가 있다면 인증 정보 없이 versioned snapshot과 refresh/close 등의 제한된 메시지만 전달한다.
- 서비스가 보고한 quota, 로컬 관측값, 계산값, 취득 불가 상태를 구분한다.

## 선택 기준

[성능 검증 계획](../performance.md)에 따라 같은 Mac·OS·release 조건과 같은 UI·provider subset을 사용한다.

1. 첫 패널 전 idle, 첫 close 후 안정화, 패널 open, 20/50회 반복, 4시간 상주를 측정한다.
2. 프로세스 트리의 physical footprint 구성과 도구의 accounting을 기록한다. RSS 단순 합산으로 총 RAM을 주장하지 않는다.
3. cold/warm click→표시 latency, 평균 CPU, idle wakeups, GPU·energy, I/O와 네트워크 요청 수를 함께 비교한다.
4. helper 방식의 idle 메모리 이점이 반복 측정에서 확인되고 첫 클릭과 반복 초기화 비용이 제품 기준에 맞으면 채택한다.
5. 차이가 측정 변동폭 수준이거나 복잡성·latency 비용이 크면 단일 프로세스를 채택하거나 설계를 조정한다.
6. 선택한 run ID, 결과, 버전/commit, 환경과 tradeoff를 이 ADR에 추가한다.

CodexBar와의 비교는 공통 provider subset에서 수행한다. claudebar는 Claude 단독 비교로 분리하고, Aura에 xAI 구독 지원이 동등하다고 가정하지 않는다. 비교 기록은 [benchmarks.md](../benchmarks.md)에 둔다.

## 근거와 적용 한계

- [claudebar main.rs](https://github.com/gautham-v/claudebar/blob/main/src/main.rs): macOS에서 native status item과 awaited click channel을 사용하고 GPUI 팝업을 필요할 때 생성·제거하는 구현 사례. 우리가 성능을 측정한 사례는 아니다.
- [Aura lib.rs](https://github.com/Rfluid/aura/blob/main/crates/aura-ui/src/lib.rs): 조사 시점에 150ms tray poll interval과 macOS hidden keepalive window가 존재했다. 이 구현을 최소 wakeup 설계의 근거로 삼지 않는다.
- [GPUI macOS window source](https://github.com/zed-industries/zed/blob/main/crates/gpui_macos/src/window.rs), [display link source](https://github.com/zed-industries/zed/blob/main/crates/gpui_macos/src/display_link.rs): 창 visibility에 따른 frame source 제어와 창 teardown 경로가 있다. 일부 display link 상태는 process lifetime 동안 유지되므로 hide/close와 process 종료를 구분한다.
- Apple [Minimize Timer Usage](https://developer.apple.com/library/archive/documentation/Performance/Conceptual/power_efficiency_guidelines_osx/Timers.html), [Extend App Nap](https://developer.apple.com/library/archive/documentation/Performance/Conceptual/power_efficiency_guidelines_osx/AppNap.html): event-driven 동작과 불필요한 timer 중단의 근거.

자료 확인일: 2026-10-25. Upstream `main`, crates.io release, 각 앱의 vendored GPUI는 서로 다를 수 있다. 실제 prototype에서 사용하는 GPUI commit을 기록하고 지원 macOS에서 검증한다.

## 확정 시 작성할 항목

- 선택한 후보: 검증 대기
- benchmark run ID와 결과 링크: 미측정
- host/helper 또는 단일 프로세스의 버전·commit: 구현 전
- 수용한 비용: 검증 대기
- 재검토 조건: close 후 메모리 회귀, helper 시작 지연, 반복 사용 에너지 증가, 지원 macOS 또는 GPUI 버전 변경

## 관련 문서

- [프로젝트 개요](../../README.md)
- [아키텍처](../architecture.md)
- [성능 목표와 검증](../performance.md)
- [비교 측정 기록](../benchmarks.md)
