---
name: Skuffen Process
description: Mandatory workflow rules for task execution, risk assessment, and planning.
---

# Skuffen Process & Governance

This skill defines the mandatory workflow for all development in Skuffen.

## Core Rules

1.  **Backlog Driven Development**
    - **Always** select the top-most active task from `.agent/tasks.md`.
    - Do not invent new tasks without adding them to the backlog first.
    - Execute tasks strictly one at a time.

2.  **Risk & Vision Awareness**
    - **Before** starting implementation, you MUST consult `.agent/vision/risk.md`.
    - Ensure your planned changes do not violate the security, data integrity, or architectural risks defined there.

3.  **Dynamic Planning & Risk Analysis**
    - If your task involves updating `.agent/vision/plan.md` or `.agent/vision/roadmap.md`, you **MUST** perform a deep risk analysis.
    - **Trigger:** Plan/Roadmap update -> **Action:** Review impact -> **Output:** Update `.agent/vision/risk.md` with new findings or mitigations.

## Workflow Summary

1.  Check `tasks.md` for next task.
2.  Read `risk.md` for constraints.
3.  Create Implementation Plan.
4.  (If Plan changes Project Plan/Roadmap) -> Update `risk.md`.
5.  Execute.
