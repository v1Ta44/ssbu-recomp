# Repository instructions

<!-- tavernweave-guidance-preference:begin schema=1 -->
TW_GUIDANCE_LEVEL=入门
用户明确选择的长期引导挡位：入门。只有用户直接要求或明确许可才能更改；临时详细/简短要求不切档。大白话与必要解释继续生效。
<!-- tavernweave-guidance-preference:end -->

## 分支约定

- `main` 跟踪 `origin/main`，是 Denkishi/ssbu-recomp 上游原始副本，保持上游状态。
- `yuzu` 跟踪 `fork/yuzu`，是用户的个人修改版本，包含暂时不想或没有计划提交上游的功能。
- `mod-adapt`、延迟滑块及其他个人功能子分支只整合到 `yuzu`。用户要求整理或合并个人功能时，默认目标是 `yuzu`。
- 除非用户明确改变上述约定，不将 `yuzu` 或个人功能子分支合入 `main`，不将整套个人功能推送到上游。
- 上游 PR 按独立授权、独立分支处理；功能在本地完成或通过验收不表示获得了提交上游的授权。
- 保留其他工作目录中的未提交修改、未跟踪调查文档与本地测试资料。
