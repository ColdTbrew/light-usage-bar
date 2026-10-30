# Light Usage Bar

macOS 메뉴 막대에서 AI 구독 사용량과 초기화 시점을 확인하는 Rust 앱이다. 막대그래프 아이콘을 누르면 제공자별 사용량 카드가 열린다.

현재 `0.1.0` 배포 후보를 개발 중이다. 대상은 Apple Silicon과 macOS 26 이상이다. 정식 릴리스나 설치 파일의 공개는 아직 완료하지 않았다. 네 제공자 전체 지원, UI 동작 완료, 저메모리 성능을 보장하는 단계가 아니다.

## 현재 구현과 검증

메뉴 막대에는 `Usage` 글자 대신 macOS의 막대그래프 아이콘을 표시한다. 아이콘을 누르면 패널이 열리고, 오른쪽 클릭이나 Control 클릭으로 제공자별 동작 메뉴를 연다.

패널은 420×560포인트 크기다. 사용률은 막대와 숫자로 표시하고 사용한 비율과 남은 비율을 구분한다. 값이 없으면 0% 대신 조회 상태를 표시한다. Claude는 `5h`와 `7d`를 별도 행으로 표시한다. 구간이 없으면 `Usage unavailable`을 표시하며 막대나 초기화 시점을 만들지 않는다. Codex의 기본 구간 이름은 응답의 실제 기간을 따른다. 각 카드의 **Details**에서 원본 구간 정보와 제공자별 동작을 연다. 제공자 목록만 스크롤하며 하단의 전체 갱신, 닫기와 종료 버튼은 고정한다.

M4 Mac mini, macOS 27.0.1에서 사용자가 제공한 이전 할당량 표시 후보 `bb810`의 일반 모드 캡처로 Claude의 두 구간, Codex의 기간과 사용률 막대, 네 카드와 고정 하단 버튼을 확인했다. 앱은 네 제공자의 합성 예시를 담은 demo도 제공하며 화면과 **Details**에 `Synthetic demo data`로 표시한다. 전체 버튼 조작과 새 demo의 화면 검증은 남아 있다. 빌드와 화면 검증의 범위는 [배포 확인 목록](docs/release.md)에 기록한다.

Codex, Grok Bot과 Antigravity의 아래 실제 계정 결과는 MacBook Pro에서 얻었다. Claude production 수집기는 Mini GUI 세션에서 검증했다.

| 제공자 | 구현 | 실제 계정 확인 결과 |
| --- | --- | --- |
| Codex | ChatGPT OAuth로 읽기 전용 사용량 조회 | 조회 성공. 한 계정에서 7일 구간 하나를 확인했다. 모든 플랜과 계정의 지원을 뜻하지 않는다. |
| Grok Bot | 공식 OIDC 인증과 WebSocket `bot.usage` 조회 | 연결, 서버 사용자 일치, 조회 성공. 관찰한 구간의 `usagePercent`가 없어 사용률은 `Unknown`이다. 기간과 초기화 시점은 있었다. |
| Claude | 공식 Claude Code CLI의 `/usage` 화면 수집 | Mini GUI 세션의 production probe에서 조회 성공. 측정값과 기간이 있는 두 구간을 수신했다. 초기화 시점은 알 수 없다. 공식 화면의 수치 대조는 미완료다. |
| Antigravity | Google 서명과 같은 사용자를 확인한 로컬 language server의 읽기 전용 HTTPS 조회 | 설치 앱 2.24.0의 status에 email이 없어 `NeedsVerification`. 실제 quota 조회 성공은 미검증이다. 앱 종료 시 `TemporarilyUnavailable`이다. |

CLI 전환 뒤 MacBook Pro에서 전체 테스트 107개, 엄격한 Clippy와 패키징 및 서명 검사를 통과했다. 새 앱의 사용자 제공 캡처에서 Claude의 `Up to date`, `5h`와 `7d` 숫자 및 막대, 알 수 없는 초기화 시점을 확인했다. Details와 버튼 수용 검사는 남아 있다. Mini에는 Antigravity 앱이 없어 해당 환경의 quota 검증도 하지 않았다.

지원 범위와 오류의 의미는 [제공자 참조](docs/providers.md)에 있다. API 결제 잔액이나 로컬 토큰 추정치를 구독 사용률로 바꾸지 않는다. 누락된 값을 0%로 표시하지 않는다.

## 빌드와 설치

Rust `1.99.1`, 전체 Xcode와 Metal toolchain이 있는 Apple Silicon Mac에서 빌드한다.

```sh
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
xcodebuild -downloadComponent MetalToolchain
bash scripts/package-macos.sh
```

GPUI 0.2.2의 기본 feature를 끈 구성에서도 `font-kit`은 명시적으로 켜야 한다. 이 feature가 없으면 macOS에서 `NoopTextSystem`을 사용해 레이아웃과 첫 프레임 응답이 정상이어도 글자를 그리지 않는다.

스크립트는 release 실행 파일 두 개를 하나의 `Light Usage Bar.app`에 넣고 ZIP과 SHA-256 파일을 생성한다. `SIGNING_IDENTITY`를 지정하지 않으면 ad hoc 서명을 사용한다. 공증은 수행하지 않는다. 이 명령의 존재는 배포 산출물 검증 완료를 뜻하지 않는다.

[설치와 패키징 안내](docs/install.md)에서 도구 준비, 출력 경로와 서명 제한을 확인한다. 인증 정보 취급은 [개인정보 설명](docs/privacy.md)을 따른다.

## 갱신과 성능

성공 후 기본 갱신 간격은 600초다. 수동 갱신도 최소 30초 간격과 `Retry-After`를 따른다. 요청은 제공자마다 하나, 전체 두 개까지 실행한다. Claude는 600초, 다른 제공자는 120초가 지난 성공 값을 오래된 값으로 구분한다. 조회 오류와 마지막 성공 값은 별도로 보관한다.

적은 상주 비용은 제품 목표다. MacBook Pro 한 대에서 패널을 열지 않은 4시간 대기 관찰 1건을 완료했다. [측정 기록](docs/benchmarks.md)에 조건과 한계를 기록했다. CodexBar 등과의 성능 비교, 50회 패널 반복과 패널 사용 후 4시간 상주 검증은 남아 있다. MVP의 성능 통과 조건은 열려 있다. 미리보기 패키지를 MVP 완료로 보지 않으며 RAM 절감률이나 에너지 우위는 주장하지 않는다.

## 문서

- [개발과 검증](docs/development.md)
- [설치와 패키징](docs/install.md)
- [현재 배포 상태](docs/release.md)
- [제공자와 오류](docs/providers.md)
- [개인정보와 인증](docs/privacy.md)
- [아키텍처](docs/architecture.md)
- [로드맵](docs/roadmap.md)
- [성능 검증 계획](docs/performance.md)과 [측정 기록](docs/benchmarks.md)
- [프로젝트 목표](docs/vision.md)와 [참고 자료](docs/references.md)

각 서비스와 독립적인 커뮤니티 프로젝트다. 공식 앱이 아니다. 라이선스는 소유자가 아직 선택하지 않았다. 공개 저장소라는 사실만으로 재배포나 상업적 사용 권한을 부여하지 않는다.
