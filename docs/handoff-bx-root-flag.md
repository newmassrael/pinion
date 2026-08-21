# 인계 — `bx --root DIR`: 링크된 워크트리에서 원격 빌드 lane 이 «닿지 않는다»

> **미인도 요청, R1759 (2026-08-21).** 대상 = `~/.claude/remote-build`(자체 git
> 저장소, 머신 전역, 51개 저장소가 공유).
> **pinion 세션에서 그 저장소는 편집 금지**(크로스 레포 하드룰)이므로 요구만
> 적는다. 우회하지 않고 보고한다.
> 심각도 **MEDIUM** — 기능은 정상이고, 잃는 것은 «원격 실행 lane 전체»다.

## 이 문서가 pinion 안에 있는 이유

처음엔 대상 저장소에 두려 했고, **그것이 그 저장소를 망가뜨린다.** 그
`.gitignore` 가 이유를 스스로 적고 있다 — `bx` 의 트리 지문에
`git status --porcelain` 이 들어가므로 **미추적 파일 하나가 양쪽 지문을 영원히
어긋나게 하고, 그 저장소를 보내는 모든 게이트가 거절한다**(2026-08-20 실측:
`bin/bx.bak-*` 하나 때문에 테스트 둘이 나흘간 red). 그래서 요구를 «내는» 쪽인
pinion 에 둔다.

## 한 줄 요약

`bx` 는 repo root 를 **현재 디렉터리**에서 정하고, 재정의할 방법이 없다:

```bash
# bin/bx:336
repo_root="$(git rev-parse --show-toplevel 2>/dev/null || true)"
[[ -n "$repo_root" ]] || repo_root="$PWD"
```

플래그는 `--label` / `--fresh` / `--no-fresh` / `--local` / `--host` 가 전부다.
그래서 **cwd 와 «빌드하려는 트리»가 다르면 원격 실행이 구조적으로 불가능하다.**

## 측정된 실패 (R1757, 2026-08-21)

작업 트리 = `/home/coin/pinion-wt/pr84`(git worktree), 셸 cwd = `/home/coin/pinion`
(에이전트 하네스가 cwd 를 메인 트리에 고정한다).

```
$ bx -- cargo check --manifest-path /home/coin/pinion-wt/pr84/Cargo.toml -p pinion-core
bx: WHERE=remote host=pc2
bx: sending tracked files to pc2:~/remote-build/pinion   ← 메인 트리를 보냈다
bx: trees agree: HEAD e8524939, working state identical  ← 메인 트리에 대해 참이라 오해를 부른다
bx: exit=101 in 17s                                       ← 그 경로가 원격에 없다
```

## 비용

그 라운드의 **빌드·테스트 18회를 전부 `--local` 로** 돌렸고, 매번 같은
`BX_LOCAL_REASON` 을 손으로 썼다. 그동안:

| | |
|---|---|
| pc2 | 32코어 중 **21코어 유휴**, 125GB 중 118GB 가용 |
| 로컬 | 한때 **load 13.16**, **swap 21GB 사용 중** |

`bx` 가 존재하는 이유(2026-08-12 의 「로컬은 아무도 결정하지 않을 때 나오는
모양」)가 워크트리에서 재현된다 — 이번엔 습관이 아니라 «다른 선택지가 없어서».
`--local` 은 정확했고 이유도 정직했지만, **그 문장이 매번 똑같았다는 것 자체가
신호다.**

## 요구 — `--root DIR`

```bash
bx --root /home/coin/pinion-wt/pr84 -- cargo test -p pinion-core
```

- `repo_root` 를 이 값으로 두고 **나머지는 지금 그대로** 파생시킨다: 전송 대상
  트리, 원격 디렉터리 이름(`basename`), 락 키, `target/bx-logs/` 위치,
  `--fresh` 의 «변경된 크레이트» 판정.
- 검증: 디렉터리이고 `git -C "$DIR" rev-parse --show-toplevel` 이 자기 자신일 것
  (워크트리도 이걸 만족한다 — 실측).
- ⚠ **원격 디렉터리 이름이 `basename` 이라는 성질이 여기서 값을 한다**:
  `pinion-wt/pr84` 는 `pr84` 가 되어 메인 트리의 `pinion` 과 **다른 원격
  디렉터리·다른 락**을 갖는다. 즉 워크트리와 메인 트리가 **동시에** 원격을 쓸 수
  있고, 그것이 워크트리 병렬 작업이 원래 원하던 것이다.
  `tools/worktree.sh` 가 이미 「이름을 저장소 간에 구별되게 지어라」를 경고로
  적고 있는데, **그 경고가 이 플래그의 전제조건**이다.

## 검토했고 버린 대안

- **`cd $DIR && bx ...`** — 하네스가 cwd 를 고정하고, 복합 명령은 가드가 거절한다
  (`bx` 자신의 헤더가 적은 이유와 같다: `bash -c` 뒤에서는 bx 가 명령을 인식하지
  못해 cargo 판정도 WRITE 판정도 눈이 먼다).
- **`--manifest-path` 만으로** — 위 실패가 그것이다.
- **워크트리를 안 쓰기** — 병렬 탐색을 포기하는 것이고, 워크트리는
  `tools/worktree.sh` 로 지원되는 워크플로다.

## pinion 쪽에서 «지금» 한 것 (R1759)

`tools/worktree.sh add` 가 붙여넣을 수 있는 stopgap 함수를 출력한다
(`wtb`, `--local` + 그 워크트리의 `--manifest-path` 를 미리 채운 것).
**우회이지 수리가 아니다** — 강제로 로컬이고 fleet 은 계속 논다. 출력이 그
한계를 명시한다.

## 수용 기준

1. `bx --root <worktree> -- cargo test -p <crate>` 가 **원격**을 고르고, 그
   워크트리의 내용을 보내고, 통과한다.
2. 「trees agree」 줄이 **그 워크트리의** HEAD/워킹 상태를 말한다.
3. 메인 트리와 워크트리가 동시에 돌아도 서로의 락·원격 디렉터리를 밟지 않는다.
4. `--root` 없이 부르면 동작이 **바이트 동일**하다.
5. 잘못된 `--root`(디렉터리 아님 / git 트리 아님)는 조용히 `$PWD` 로 떨어지지
   말고 **거절**한다 — 조용한 폴백이면 이 결함이 이름만 바뀐 채 남는다.

## 프로세스 주의

대상 저장소는 **읽기만** 했다(`bin/bx` 의 플래그 파싱과 `repo_root` 해석,
`.gitignore`, `git log`). 어떤 파일도 수정하지 않았고, 한 번 잘못 만든 파일은
즉시 지워 그 저장소를 원상 복구했다.
