# Contributing to RedDuck Academy

Thanks for helping. There are a few ways in, and none of the content ones need you to run the app
or a database.

## Improve or add a lecture (no setup)

The lectures are Markdown files under [`content/`](content/). Fixing a typo, sharpening an
explanation, correcting a code sample, or adding a lesson is a normal pull request.

➡️ Full guide: **[content/README.md](content/README.md)**

Before you push, you can catch problems early (CI runs the same check on your PR):

```bash
node scripts/validate-content.mjs
```

## Report or propose without writing a PR

Open an [issue](../../issues/new/choose) and pick the form that fits:

- **Improve a lesson** — something is wrong or unclear in an existing lecture.
- **Propose a new lesson** — a topic you think belongs in a course.
- **Bug report** — the site or the platform code is broken.

## Run or self-host the platform

The web app, backend, and CMS are covered in **[docs/self-hosting.md](docs/self-hosting.md)**. You
don't need any of this to contribute lectures.

## Pull requests

- Keep the change focused: one topic per PR.
- The PR template has a short checklist for content changes.
- For a new lesson file, leave out the `id` — one is assigned automatically on merge — and
  keep the `---` frontmatter block intact.

## Licensing of contributions

RedDuck Academy is dual-licensed: platform code under **MIT**, lecture content (`content/`) under
**CC BY-SA 4.0**. By contributing, you agree your contribution is provided under the same license as
the part of the project you are changing.
