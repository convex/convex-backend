# Hybrid Convex Components

Read this file only when the user explicitly wants a hybrid setup.

## What This Means

A hybrid component combines a local Convex component with shared library code.

This can help when:

- the user wants a local install but also shared package logic
- the component needs extension points or override hooks
- some logic should live in normal TypeScript code outside the component
  boundary

## Default Advice

Treat hybrid as an advanced option, not the default.

Before choosing it, ask:

- Why is a plain local component not enough?
- Why is a packaged component not enough?
- What exactly needs to stay overridable or shared?

If the answer is vague, fall back to local or packaged.

## Risks

- More moving parts
- Harder upgrades and backwards compatibility
- Easier to blur the component boundary

## Checklist

- [ ] User explicitly needs hybrid behavior
- [ ] Local-only and packaged-only options were considered first
- [ ] The extension points are clearly defined before coding
