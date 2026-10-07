# W08 Bundled Skill Helpers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用标准库离线回归覆盖已捆绑的 claim/path、文献失败语义及 figure-style 行为，并加入 Windows CI 的统一 Python discovery。

**Architecture:** 测试只加载仓库中三个固定 runtime.py 到隔离 namespace，直接断言 helper，HTTP/等待/可选绘图库使用最小替身。此计划只拥有 Python discovery/setup-python；V4 manifest、runner、JSON report/upload 由 runtime 场景计划拥有，不产生第二套报告。

**Tech Stack:** Python 3.12/stdlib unittest、unittest.mock/urllib/types/io/tempfile；GitHub Actions Windows/Node.js 22。无 pip/scientific libraries、新 CLI、production API 或锁文件更新。

**Spec:** [W08 helper 设计](../specs/2026-10-04-wisp-diagnostics-example-design.md#w08已导入-skill-helper-的行为回归与共同-ci-边界)、[Runtime W08 runner](../specs/2026-10-04-wisp-runtime-design.md#w08manifest-驱动的离线-v4-场景与稳定报告)、[总设计](../specs/2026-10-04-wisp-improvements-design.md)。设计基线 `161199f`。

## Global Constraints

- 只加载 `skills/wisp-science/paper-narrative/runtime.py`、`literature-review/runtime.py`、`figure-style/runtime.py`，三个完整路径共享同一固定目录前缀；不执行用户安装或上游下载代码。
- 使用新 `__name__="__main__"` namespace、清空环境后注入假 key/contact，所有 sys.modules/mock/env 修改恢复；load 不出 stdout、不联网/等待/写文件。
- 捆绑源码、SOURCE pin、hash/冻结逻辑不变；不为制造 RED 修改 helper。已有正确行为首跑 PASS 时如实记录，必要的新最小 loader/fixture 尚未实现时可记录真正 FAIL。
- `panel_crops` 只返回像素坐标，不 import PIL、不写/裁切图片；测试不得发明 crop 清理或科学图像执行语义。
- CI 保留 Windows、Node 22、npm ci、workspace/npm/build 顺序；增加 setup-python@v5 Python 3.12 与 stdlib discovery，无 credentials/pip/真实网络/SSH/GPU/R。
- helper unittest 输出不并入 V4 call count/suite hash。共享 CI 文件按 master 顺序合并，report/upload 由 runtime plan 负责。

## Review Focus

1. 动态名字/缺 runtime.py、环境真 key 泄漏或 load 副作用：固定 allowlist、缺失失败、清空环境、重导入和 mock 恢复（Task 1）。
2. Unicode/引号/Windows 路径、null/缺失 path：claim 与同条 path 保持绑定，不改输入、不补造（Task 1）。
3. OpenAlex empty 与错误、坏 JSON/results、429 次数：失败不是零结果，重试一次、无真实 sleep（Task 2）。
4. DOI/head 未知、引文单向失败、完整 traceback含 URL/key：unknown 保留、失败整体传播、无秘密/原异常（Task 2）。
5. 已有 matplotlib backend、共享轴边界与 discovery失败传播：load 不改全局，坐标合并/clamp正确，CI 非零不掩盖（Tasks 1、3）。

---

### Task 1: 最小安全 loader、claim 与 figure-style 合同

**Files:** Create `scripts/test_bundled_skill_helpers.py`; Test 同一文件的 unittest classes；捆绑的三个 runtime.py 只读。

**Interfaces:**
- Consumes: 本地现有 `_paper_figure_claims_json(figures)`、`paper_brief_task(abstract_text, figure_claims)`、`figure_style_self_check()`、`panel_crops(fig, dpi=None, pad_px=6, bbox_inches=None, pad_inches=None)`。
- Produces: 仅测试内 `load_bundled_helper(skill_name: str) -> dict[str, object]`（固定三名字映射）；`make_panel_fixture(shared_axis: str, same_slot: bool) -> tuple[types.SimpleNamespace, dict[str, object]]` 返回 fake fig/可注入 matplotlib modules。小型 unittest fixture 足够，不新增多层 runner/插件框架。

- [x] **测试 — 写 `BundledHelperLoadTests`、`PaperNarrativeTests`、`FigureStyleTests`。** loader 新接缝缺失时是真 RED；helper 直接断言包括输入 deep-copy 不变、显式 null/缺失path、Unicode/引号/Windows 路径 round-trip，load 在 __main__ 下无 heavy import/rcParams/backend/stdout/网络/sleep/写入。stub 已安装 matplotlib 时也不改变 backend；显式 self_check 才应用样式。panel 坐标验证顶部原点、边界 clamp、同排 shared-y/同列 shared-x 合并、不同 row/column 不合并。

  ```python
  rows = json.loads(namespace["_paper_figure_claims_json"](figures))
  self.assertEqual(rows[0]["composite_path"], figures[0]["composite_path"])
  self.assertIsNone(rows[1]["composite_path"])
  self.assertNotIn("composite_path", rows[2])
  self.assertEqual(figures, original_figures)
  self.assertEqual(crops["a"], expected_clamped_box)
  blocked_write.assert_not_called()
  ```

- [x] **RED/基线 — 运行并记录实际输出。** `python -B -m unittest discover -s scripts -p "test_bundled_skill_helpers.py" -v`；新 loader 未实现预期 FAIL/NameError。若直接 helper 合同已 PASS 则记录 PASS，不虚称修复缺陷、不改 helper 制造失败。
- [x] **实现必要测试签名/替身。** 源先由固定 Path 读入，再以 compile/exec 在新 namespace 载入；拦截仅不允许的网络/写/可选库，允许 stdlib imports。用 unittest.mock patch.dict 清空 env 与恢复 sys.modules，fake Bbox/axes/render 只实现现有 helper 用到的几何接口。无图像/科学依赖安装。
- [x] **GREEN — 同命令 PASS。** 再用测试 namespace 中一次错配 claim/path 或 load-side-effect mutation 验证直接 contract assertion 产生预期失败，由外层测试断言捕获；源文件不变，不增加故障注入运行时框架。缺 helper 直接失败，无 skip。所有临时测试文件在 tempdir，`-B` 不写 pyc。
- [x] **Commit —** `git add -- scripts/test_bundled_skill_helpers.py`；`git commit -m "test: cover bundled claim and figure helper contracts"`。

### Task 2: 文献检索/引文/DOI 错误与秘密回归

**Files:** Modify `scripts/test_bundled_skill_helpers.py`; Test `LiteratureReviewTests`。

**Interfaces:**
- Consumes: Task 1 loader；本地 `search_openalex(query, n=10, filters="")`、`expand_citations(doi, n_backward=50, n_forward=15)`、`verify_dois(dois)`、`_get_json(url, timeout=15, strict=False)`。
- Produces: 测试内 `FakeHttpResponse(payload: bytes, status: int = 200)`，只提供 read/context-manager/status；HTTPError/URLError 用 stdlib 对象，urlopen/build_opener/open/time.sleep 直接 patch。不增加产品 helper 或独立 report。

- [ ] **测试 — 写文献行为矩阵。** success-empty/populated、HTTP 401/404/429/500、超时/URL error、坏 UTF-8/JSON、results 非list/row非dict；429 重试一次。expand 验证 references 用 cited_by、cited_by 用 cites，两方向均成功才允许 empty，任一失败不返回 partial。CrossRef miss/doi HEAD unavailable 返回 ok=None，404返回False、2xx/3xx返回True；假 OPENALEX key 只到该服务。

  ```python
  self.assertEqual(search_openalex("x"), [])  # fake 成功零结果
  with self.assertRaises(RuntimeError) as caught:
      search_openalex("x")  # fake HTTP 500
  self.assertNotIn("FIXTURE_SECRET", "".join(traceback.format_exception(caught.exception)))
  self.assertEqual(urlopen.call_count, 2)
  sleep.assert_called_once_with(2)
  self.assertIsNone(verified[doi]["ok"])
  ```

- [ ] **RED/基线 — 运行实际结果。** `python -B -m unittest discover -s scripts -p "test_bundled_skill_helpers.py" -v`；必要的新 FakeHttpResponse 未实现时 FAIL；现有 helper 正确分支可能已经 PASS，如实记录。
- [ ] **实现最小 HTTP fixture 与直接断言。** response/exceptions 在 patch side_effect 声明，未 mocked network 一律会失败；fake key/contact 在 clear=True 环境下，assert 原异常/请求 URL/secret 未进入完整 traceback。把吞 HTTP error 返回 [] 的假函数仅放入局部 namespace，确认合同 assertion 拒绝，再恢复原函数。无需修改已正确的 runtime.py。
- [ ] **GREEN — 同命令 PASS。** 所有矩阵无真实等待/网络，mock退出后 environment/urlopen/sys.modules恢复；失败可见，不靠 broad except/skip 将断言吞掉。运行现有安装/完整 hash 两测试 `cargo test -p omicsops-desktop --lib wisp_snapshot`，记录实际通过数，不得称零测试通过。
- [ ] **Commit —** `git add -- scripts/test_bundled_skill_helpers.py`；`git commit -m "test: distinguish literature retrieval failures from empty results"`。

### Task 3: 统一 Python discovery、CI 与证据说明

**Files:** Modify `.github/workflows/ci.yml`、`scripts/README.md`、`docs/bundled-science.md`、`acceptance/README.md`; Modify `scripts/test_bundled_skill_helpers.py`（仅 discovery/nonzero smoke）；不修改 runtime scenario report/upload 步骤。

**Interfaces:**
- Consumes: Tasks 1–2 的 unittest classes；现有 `scripts/test_import_model_catalog.py`、`test_remote_job_bridge.py`；W04 增加的 `scripts/test_kernel_inspection.py` 也由同一标准 discovery 自动纳入，不需该计划重复步骤。
- Produces: CI `actions/setup-python@v5` / `python-version: '3.12'`，统一命令 `python -B -m unittest discover -s scripts -p "test_*.py"`；无 custom report、pip 或 npm test Python前提。CI 保留现有 required checks，其同文件合并按 master 顺序执行。

- [ ] **测试 — 写 `HelperDiscoveryTests`。** 用 unittest TestLoader 从固定本地 tests 加载后断言三组 helper cases 可发现；本测试中的独立 TestCase 设置故意失败断言，以 TestResult 验证 failed>0，或受控 subprocess 对该临时 case 验证 exit nonzero，不运行产品/network，也不让正常 suite 故意失败。测试缺失文件/类不能以 empty suite 当成功。

  ```python
  self.assertGreater(discovered.countTestCases(), 0)
  self.assertFalse(injected_failure_result.wasSuccessful())
  self.assertEqual(len(injected_failure_result.failures), 1)
  ```

- [ ] **RED/基线 — 运行并记录实际结果。** `python -B -m unittest discover -s scripts -p "test_*.py" -v`；新 smoke 未实现时记录 FAIL，已存在 unittest 正确行为首跑即 PASS 时记录事实；当前 CI 尚无 Python step 是待实现配置，不伪造运行失败。
- [ ] **实现 CI 两个新增步骤和 smoke/docs。** 保留 npm ci→workspace→npm test→build 的顺序；Python discovery 置 npm ci 后、workspace 前，setup-python 在其前。step不加 continue-on-error 或 always-success；仅 runtime owner 管理 OMICSOPS_EVAL_REPORT_PATH 和 upload-artifact。docs 明确 bundle/hash、helper PASS、scripted V4 PASS 与 live acceptance 未执行各自证明什么。
- [ ] **GREEN — discovery PASS，默认完整检查 PASS。** `cargo test --workspace`、`npm test`、`npm run build`、`git diff --check`；纯测试/CI变化无桌面打包必要，若整合时改宿主资源则按 master 跑桌面检查。用 stdlib环境运行，记录 case数量/零skip；确认三个 runtime.py/SOURCE hash未变。CI实际执行状态未发生时注明“本地通过，CI未执行”，不把修改YAML当已通过CI。
- [ ] **Commit —** `git add -- scripts/test_bundled_skill_helpers.py .github/workflows/ci.yml scripts/README.md docs/bundled-science.md acceptance/README.md`；`git commit -m "ci: run deterministic bundled skill helper regressions"`。

## 当前实施证据

2026-10-08：Task1提交74e8fa4独立spec/quality审阅通过，无open finding。仅新增307行标准库测试；固定loader与claim/path、figure几何合同focused14通过，完整Python discovery23通过。实际loader缺接缝RED为1个NameError；首轮helper11通过/1个Windows platform fixture失败，stub后原helper合同baseline PASS，没有修改helper制造缺陷。两次sandbox discovery均保留5个既有RemoteJobBridge WinError5，原命令授权重跑后23通过。runtime.py/SOURCE/hash/freeze不变；外部未提交工作完整保留。实际本地Python3.10，计划CI3.12未执行；本leaf未重复Rust/npm/Web/native。Task2文献矩阵与Task3discovery/CI/完整交付待实施；W08场景report/upload仍由另一子计划之后拥有，整个W08尚未完成。
