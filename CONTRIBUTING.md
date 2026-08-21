# 贡献指南（Git 协作规范）

本仓库按**多人协作**原则管理，规则同时约束人类与 AI 助手。

## 分支模型（GitHub Flow）

- `main` 是**唯一发布干线，永远可发布**，受分支保护：**禁止直接 push**、要求 PR + 1 个审批。
- 所有改动走短命特性分支，命名：`feat/<描述>` / `fix/<描述>` / `refactor/<描述>` / `chore/<描述>`。
- 合并策略为 **squash merge**（PR 折叠为单个提交）。合并后分支自动删除。

## 提交规范（Conventional Commits）

```
feat(desktop): 新增 Tauri 桌面版鲸鱼
fix(ledger): 修复跨天归零时 lastBalance 未重置
chore(ci): 增加 cargo 静态检查
```

- 一条提交只包含一个逻辑单元。
- 推送前可用 `git rebase -i` 整理提交，但**绝不修改已合入 main 的历史**。
- 提交信息用中文 + 前缀，描述清楚「改了啥、为什么」。

## PR 流程

- PR 标题 = 变更说明；描述写清：改动内容 / 动机 / 验证方式。
- 大改动拆多个小 PR，每个 PR 聚焦一件事。
- **main 必须保持全绿**：`check.yml` 通过才可合入。
- 自行审查：看 `git diff --check`（空白错误）、无遗留调试代码、无硬编码密钥。

## 工作流习惯（多人同仓库）

1. 开工前：`git fetch origin && git switch main && git pull`。
2. 从最新 main 切新分支，避免 stale branch 冲突。
3. 提交节制：能看懂、能回滚。
4. 合并交给 PR 审查，不直接 `git push origin main`。

## 严禁事项

- ❌ 直接 push `main`
- ❌ force push（含 `git push -f`）
- ❌ 修改已合入 main 的历史提交（rebase/amend）
- ❌ 把秘密/密钥提交进仓库（API Key 走本地配置文件，绝不入库）
- ❌ 绕过分支保护（临时关保护要留痕并在事后恢复）

## CI

| workflow | 触发 | 内容 |
|---|---|---|
| `check.yml` | 每次 PR | desktop：JS 语法检查 + `cargo fmt/clippy/test` |
| `publish.yml` | push main | 自动发布 dsh-whale-widget 到 npm |
| `release-desktop.yml`（规划中） | tag `v*` | Tauri 桌面版打包并发布 GitHub Release |