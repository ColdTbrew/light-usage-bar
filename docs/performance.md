# 성능 목표와 검증 계획

상태: **macOS 앱 미구현·앱 성능 미측정**

갱신일: 2026-10-29

대상: macOS 전용. Apple Silicon에서 먼저 측정하며 Intel 결과는 실제 측정한 경우에만 별도로 게시한다.

## 확인한 측정 환경

2026-10-29에 tailnet SSH로 아래 환경을 확인했다. 앱 성능 측정 결과는 아니다.

| 항목 | 확인한 값 |
| --- | --- |
| 모델 | MacBook Pro, `Mac15,6` |
| 칩 | Apple M3 Pro, ARM64 |
| RAM | 18 GiB, `hw.memsize = 19327352832` |
| macOS | `26.6.2`, 빌드 `25G83` |
| 개발 도구 | Command Line Tools, `/Library/Developer/CommandLineTools` |
| 기본 측정 도구 | `/usr/bin/vmmap`, `/usr/bin/footprint`, `/usr/bin/powermetrics`, `/usr/bin/sample`, `/usr/bin/time` |

도구의 경로 존재를 확인했다. 앱을 대상으로 한 수집, 권한, sampling 조건은 실제 실험 때 확인한다. 전체 Xcode와 Instruments의 사용 가능 여부도 아직 검증하지 않았다.

## 1. 제품 목표

Light Usage Bar의 핵심 목표는 항상 실행해도 부담이 작은 AI 구독 사용량 메뉴 막대 앱이다. **CodexBar 등 다른 사용량 앱과 RAM을 비교하고, 조건과 결과를 공개하는 것**도 프로젝트 목표에 포함한다.

Rust, GPUI, 별도 helper라는 선택만으로 저메모리·저전력을 주장하지 않는다. 시작 직후 RAM, 패널을 한 번 열었다 닫은 뒤의 상주 RAM, 패널이 열린 동안의 전체 사용량을 각각 확인한다. GPUI helper 방식은 우선 검증 후보이며 [단일 프로세스 후보](decisions/0001-ui-process-lifecycle.md)와 비교한 뒤 확정한다.

현재 특정 MiB 이하라는 제품 약속이나 타 앱보다 몇 % 적다는 주장은 없다. 초기 prototype 측정으로 예산을 정하고, 이후 release 검증에서 그 예산과 회귀 기준을 적용한다.

## 2. 자원 사용을 줄이는 설계 규칙

| 영역 | 설계 규칙 | 확인 방법 |
| --- | --- | --- |
| 상주 UI | AppKit `NSStatusItem`의 표시가 바뀔 때만 업데이트 | status update counter와 trace |
| 이벤트 | click/focus/file 변경은 callback 또는 channel로 처리 | 상시 100~150ms poll loop가 없는지 source 및 wakeup 검토 |
| 패널 | 클릭할 때만 GPUI 표시. 닫으면 renderer 수명 종료 시도 | window/helper 수명과 close 후 footprint 측정 |
| UI 작업 | 닫힌 동안 countdown·animation·바깥 클릭 monitor 제거 | timer, event monitor, render counters |
| 배경 조회 | 기본 10분, 하나의 스케줄러, tolerance와 batching | 실제 provider별 요청 횟수 |
| 실패 | `single-flight`, timeout, backoff, `Retry-After` | offline/401/429 시나리오 |
| 프로세스 | 브라우저와 provider CLI 상주 없음 | 프로세스 트리와 자식 종료 확인 |
| 파일·네트워크 | 증분 읽기, 크기 상한, cache 우선, 불필요한 write 금지 | bytes read/written, 요청 크기·횟수 |

App Nap은 보조 장치다. 앱 자체가 작업을 줄여 idle에 도달해야 하며, 특정 상태에서 App Nap이 작동할 것이라고 전제하지 않는다. 사용자에게 보이는 데이터의 신선도와 10분 갱신의 tradeoff를 UI에 드러낸다.

## 3. 타 앱과 비교하는 조건

### 비교 대상

| 대상 | 역할 | 비교 범위 |
| --- | --- | --- |
| [steipete/CodexBar](https://github.com/steipete/CodexBar) | 주된 다중 provider baseline | 양쪽에서 같은 의미의 quota를 지원하는 provider만 동일하게 켜서 비교 |
| [gautham-v/claudebar](https://github.com/gautham-v/claudebar) | Claude 단독 baseline | Light Usage Bar도 Claude만 켠 별도 실험 |
| [Rfluid/aura](https://github.com/Rfluid/aura) | Rust + GPUI 다중 provider 참고 | 실제 지원하는 공통 subset만 비교. xAI Bot 지원이 동등하다고 가정하지 않음 |
| [akitaonrails/ai-usagebar](https://github.com/akitaonrails/ai-usagebar) | 선택적 Rust 기반 비교 대상 | 해당 release의 macOS tray/WKWebView 경로와 기능을 확인한 뒤 포함 |

이름이 비슷한 `tddworks/ClaudeBar`와 `gautham-v/claudebar`를 혼동하지 않는다. 모든 대상은 **실제로 실행한 버전 또는 commit**을 기록한다. README의 “lightweight” 표현은 측정 결과가 아니다.

### 통제 조건

- 같은 Mac, macOS build, 전원 상태, 화면 수·해상도·배율, 동일한 foreground workload에서 순차 실행한다.
- Release build를 사용한다. 서명·공증 여부, 빌드 옵션, 배포 바이너리 또는 자체 빌드 여부를 기록한다. Debug와 release를 섞지 않는다.
- 같은 provider 수, 같은 인증 상태, 가능한 같은 quota window와 데이터량, 같은 refresh cadence를 사용한다. 설정을 맞출 수 없으면 차이를 표에 적고 동등 비교에서 제외하거나 별도 결과로 낸다.
- 첫 설정·OAuth 로그인은 측정 구간 전에 끝낸다. 재인증 화면이나 브라우저 실행 비용은 별도 시나리오로 측정한다.
- 앱 하나씩 측정한다. 실험 순서를 바꿔 반복하고 cache의 cold/warm 조건을 기록한다.
- 네 provider 전체를 켠 자체 실험과 공통 subset 실험을 구분한다. **Claude 하나만 켠 앱의 RAM을 네 서비스 앱과 직접 비교하지 않는다.**
- CPU·디스크를 쓰는 다른 앱, OS update, Spotlight 작업 등 교란 요인이 있으면 기록하고 해당 run을 재실행한다.

서비스 quota는 시간에 따라 변한다. UI 수명과 cache만 비교하는 deterministic fixture 실험, 실제 인증·네트워크를 포함한 end-to-end 실험을 나눠 기록한다. 모의 데이터 결과를 실제 연동 성능처럼 발표하지 않는다.

## 4. 수집 지표와 해석

| 지표 | 기록 방식 | 주의점 |
| --- | --- | --- |
| Process physical footprint | host, GPUI helper, 앱이 실행한 probe 자식별 평균·p95·peak | startup과 steady state를 구분 |
| Process-tree footprint | 같은 시점에 앱 프로세스와 자식의 physical footprint를 함께 보고한 합계 및 구성 | 도구의 accounting과 중복 가능성을 명시. 기기 전체의 유일한 물리 메모리 증가량이라고 부르지 않음 |
| RSS | 필요 시 보조 진단값 | **RSS 단순 합산은 shared pages를 중복 계산하므로 총 RAM headline으로 사용하지 않음** |
| CPU | 구간 평균·p95·총 CPU time, percent 정의 | 한 logical core 기준인지 도구 정의를 명시 |
| Idle wakeups | 초당 평균과 timer 원인 | 순간 CPU 0.0%만으로 저전력을 판단하지 않음 |
| GPU·energy | 가능한 도구의 GPU 활동, Energy Impact, 필요 시 power trace | Energy Impact 점수는 watts가 아님 |
| UI latency | click→첫 유효 snapshot 표시의 median·p95 | 첫 실행과 warm 실행, cached 데이터와 새 fetch 완료를 구분 |
| I/O·network | bytes, 요청 횟수, 429 retry, 로그 스캔량 | 평균 RAM이 낮아도 과도한 wakeup·I/O면 재검토 |
| 수명 | helper 시작·종료 시각, 자식 수, 열린 창 수 | 닫힌 helper를 측정에서 누락하지 않음 |

Apple Silicon은 CPU와 GPU가 unified memory를 공유한다. renderer/texture 비용이 도구에서 어떻게 귀속되는지 기록하고, 이미 포함된 GPU 항목을 중복 가산하지 않는다. 필요하면 `vmmap`, Instruments, 시스템 memory pressure를 함께 확인한다. 메모리를 해제했어도 allocator 또는 driver cache 때문에 footprint가 즉시 줄지 않을 수 있으므로 측정 시점을 명시한다.

측정 도구 자체도 자원을 사용한다. baseline과 모든 후보에 같은 sampling rate와 도구를 적용한다. Activity Monitor는 관찰 보조로 사용하고, 비교 표에는 재현 가능한 구간 통계를 남긴다.

## 5. 재현 가능한 benchmark protocol

초기 spike는 각 후보를 최소 3회 측정한다. 재현성 문제가 보이면 원인을 해결한 뒤 반복 횟수를 늘린다. 아래 시간과 횟수는 **실험 규칙**이며 성능 수치가 아니다.

| 시나리오 | 절차 | 확인할 것 |
| --- | --- | --- |
| Cold launch | 같은 초기 cache 조건에서 실행. 첫 60초와 안정화 후를 분리 | 초기화 peak, 첫 fetch, 시작 RAM |
| 첫 패널 전 idle | startup 작업 종료 후 패널을 열지 않고 10분 관찰 | host steady footprint, wakeups, provider fetch 비용 |
| 패널 open | cached snapshot으로 열고 60초 정적 표시 | host+helper footprint, GPU 활동, UI latency |
| 첫 close 후 idle | 닫은 후 즉시, 30초, 2분, 10분 시점 기록 | helper 종료, renderer 해제, 시작 전 대비 남는 메모리 |
| 반복 open/close | 20회, 이어 총 50회. 매회 3초 open/3초 closed; cold/warm 별도 기록 | 누적 성장, peak, helper 중복, focus/monitor 누수 |
| 장시간 상주 | 4시간 이상. 사용 패턴과 조회 시점을 기록 | RSS/footprint 추세, CPU time, 요청·로그 증가, 초기 cache warmup 후 안정성 |
| Sleep/wake | 최소 3회 반복. wake 시 네트워크 복구 지연도 포함 | timer catch-up 폭주, stale 표시, 한 번의 필요한 갱신 |
| 오류·제한 | offline, timeout, 401, 429 + `Retry-After`를 제어된 fixture로 재현 | retry 간격, CPU/로그 폭주 없음, 다음 시도 시각 유지 |
| Probe 종료 | 느린 응답·정지 subprocess·helper crash를 주입 | deadline, 취소, 자식 cleanup, host 생존 |

실제 provider에 고의로 과도한 요청을 보내 429를 유발하지 않는다. 오류 주입 fixture는 네트워크 client의 경계에서 재현하고, 실제 provider 연동 결과와 구분한다.

## 6. 통과 기준

### 설계 선택 전

1. 동일 UI·provider fixture로 **helper와 단일 프로세스**의 필수 상태를 모두 측정한다.
2. idle RAM이 줄었는지 host만 보지 않고 helper·probe를 포함한 프로세스 트리로 판단한다.
3. 패널 첫 표시 latency와 반복 open/close의 CPU time·에너지를 함께 비교한다.
4. 선택 이유와 재현 가능한 측정 run을 ADR에 연결한다. RAM 차이가 작거나 불확실하면 복잡성이 낮은 후보를 우선 재검토한다.

### MVP release 전

| Gate | 판정 기준 |
| --- | --- |
| 결과 공개 | 필수 상태의 측정값·환경·버전·시나리오·공통 provider subset이 [benchmarks.md](benchmarks.md)에 있음 |
| 닫힌 UI | app이 요청하는 animation/countdown/클릭 polling 작업이 없음. 바깥 클릭 monitor도 해제됨 |
| Helper 수명 | 선택한 helper 방식에서 close 후 2초 내 정상 종료하는 것을 목표 gate로 검증. 실패하면 원인과 수정 결과 기록 |
| Probe 제한 | provider/account당 in-flight 1개, 전체 초기 상한 2개. 모든 probe에 deadline. timeout 뒤 자식이 남지 않음 |
| Retry | 429의 `Retry-After` 이전 재요청 0회. 연속 refresh 클릭 및 sleep/wake로 제한이 풀리지 않음 |
| 메모리 안정성 | 50회 반복 및 4시간 실험에서 설명되지 않는 지속 증가가 없음. cache 상한·해제 근거를 함께 검토 |
| 회귀 | 최초 승인 baseline 이후 같은 환경의 steady footprint 또는 p95 open latency가 10% 넘게 악화하면 재측정·원인 검토. 변동폭 이하 차이는 개선으로 발표하지 않음 |
| 비교 주장 | “CodexBar보다 RAM이 적다”는 표현은 같은 조건의 반복 측정이 뒷받침하는 해당 상태·subset에만 한정 |

2초·10% 등은 **제안된 검증 기준**이며 달성 결과가 아니다. 수치 변경은 근거와 함께 기록한다. 절대 RAM 예산은 spike가 나오기 전 임의로 약속하지 않는다. 후보가 목표를 충족하지 못하면 프로세스 수명·cache·renderer 정책부터 조정한다.

## 7. 현재 결과

| 후보/비교 대상 | 첫 패널 전 idle | 첫 close 후 idle | 패널 open | 50회 후 | 4시간 후 |
| --- | --- | --- | --- | --- | --- |
| Light Usage Bar: host + helper | 미측정 | 미측정 | 미측정 | 미측정 | 미측정 |
| Light Usage Bar: 단일 프로세스 | 미측정 | 미측정 | 미측정 | 미측정 | 미측정 |
| CodexBar: 공통 provider subset | 미측정 | 미측정 | 미측정 | 미측정 | 미측정 |
| claudebar: Claude 단독 subset | 미측정 | 미측정 | 미측정 | 미측정 | 미측정 |
| Aura: 공통 provider subset | 미측정 | 미측정 | 미측정 | 미측정 | 미측정 |
| ai-usagebar: 선택적 비교 | 미측정 | 미측정 | 미측정 | 미측정 | 미측정 |

이 표는 계획 상태다. 결과의 단일 기록 위치는 [benchmarks.md](benchmarks.md)로 두고, 공개 데이터에서 이메일·계정 ID·workspace 이름·인증 정보·대화 내용을 제거한다.

## 8. 근거 자료

확인일: 2026-10-25.

- Apple, [Minimize Timer Usage](https://developer.apple.com/library/archive/documentation/Performance/Conceptual/power_efficiency_guidelines_osx/Timers.html): event notification, 불필요한 timer 중지, tolerance, wakeup 조사.
- Apple, [Extend App Nap](https://developer.apple.com/library/archive/documentation/Performance/Conceptual/power_efficiency_guidelines_osx/AppNap.html): App Nap에 의존하지 않고 앱 스스로 idle 상태에 도달해야 한다는 원칙.
- Apple, [Monitor Energy Usage](https://developer.apple.com/library/archive/documentation/Performance/Conceptual/power_efficiency_guidelines_osx/MonitoringEnergyUsage.html): CPU, Energy Impact, I/O 등 복수 지표 확인.
- GPUI, [Window API](https://docs.rs/gpui/latest/gpui/struct.Window.html), [macOS window lifecycle](https://github.com/zed-industries/zed/blob/main/crates/gpui_macos/src/window.rs), [Metal renderer](https://github.com/zed-industries/zed/blob/main/crates/gpui_apple/src/metal_renderer.rs): redraw, 창 수명, GPU 자원 구조. 버전별 구현은 달라질 수 있음.

## 관련 문서

- [프로젝트 개요](../README.md)
- [아키텍처](architecture.md)
- [비교 측정 기록](benchmarks.md)
- [ADR 0001: UI 프로세스 수명](decisions/0001-ui-process-lifecycle.md)
