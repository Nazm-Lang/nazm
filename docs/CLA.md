# Nazm Contributor Licence Agreement

**Status: draft, version 0.1 (2026-10-11).** Chosen by the copyright holder on 2026-10-11 as
Nazm's contributor terms ([`licensing.md`](licensing.md)). This text was written without legal
review; it is to be reviewed by a lawyer before the first outside contribution is accepted, and
no contribution is accepted under it before then.

This agreement is between **you** — the individual, or the legal entity on whose behalf an
individual signs — and **Ferdous Alam**, the holder of Nazm's copyright, together with any
successor or assignee to whom Ferdous Alam transfers the Nazm project (**the Maintainer**). It
covers every Contribution you submit to the Nazm project, now or later, until you end it under
section 8.

## 1. Definitions

- **Contribution** — any work of authorship, including any change to an existing work, that you
  intentionally submit to the Maintainer for inclusion in Nazm: code, documentation, tests,
  examples, designs and their accompanying data, through a pull request, patch, issue, comment or
  any other channel the project uses. A work you mark in writing as *"Not a Contribution"* when you
  submit it is not one.
- **Nazm** — the Nazm language, its specification, toolchain, standard library, runtime and the
  other works the Maintainer publishes as part of the project.

## 2. What you keep

You keep the copyright in your Contributions. This agreement grants licences; it does not
transfer ownership.

## 3. Copyright licence

You grant the Maintainer and every recipient of software or documentation the Maintainer
distributes a perpetual, worldwide, non-exclusive, no-charge, royalty-free, irrevocable licence to
reproduce, prepare derivative works of, publicly display, publicly perform, sublicense and
distribute your Contributions and such derivative works.

**Relicensing.** The Maintainer may distribute and sublicense your Contributions under the
licence Nazm uses at the time you contribute, under any other licence — open-source or
proprietary, with or without payment — and under several at once. Whatever the Maintainer does,
your Contribution as you submitted it stays available under the licence Nazm used when you
submitted it.

## 4. Patent licence

You grant the Maintainer and every recipient of software the Maintainer distributes a perpetual,
worldwide, non-exclusive, no-charge, royalty-free, irrevocable (except as stated in this section)
patent licence to make, have made, use, offer to sell, sell, import and otherwise transfer your
Contributions, where the licence covers only the patent claims licensable by you that are
necessarily infringed by your Contribution alone or by its combination with the work to which you
submitted it. If any entity starts patent litigation against you or any other entity — including
a cross-claim or counterclaim in a lawsuit — alleging that your Contribution, or the work to which
you contributed, infringes a patent, every patent licence granted to that entity under this
agreement for that Contribution or work ends on the date the litigation is filed.

## 5. What you promise

1. You are legally entitled to grant these licences. If your employer or anyone else has rights in
   what you create — including your Contributions — you have their permission to contribute on
   their behalf, or they have waived those rights, or they have signed this agreement as an entity.
2. Each Contribution is your original creation, except for material you identify under
   section 6.
3. Your Contribution includes, in its description, every third-party licence or other restriction
   you personally know of — including any patent and any licence term — that applies to any part
   of it.
4. If part of a Contribution was produced by a generative tool, you say so when you submit it, you
   reviewed that part, and to your knowledge it reproduces no third party's work that you could not
   submit under section 6.

## 6. Material that is not yours

If you want to submit work that is not your original creation, submit it separately from any
Contribution, name its source and its licence, and mark it conspicuously as third-party material,
for example *"Submitted on behalf of a third party: [name]"*.

## 7. No other obligations

You are not expected to support your Contributions, except where you choose to. Unless required
by applicable law or agreed in writing, you provide your Contributions **"as is", without
warranties or conditions of any kind**, express or implied, including any warranty or condition of
title, non-infringement, merchantability or fitness for a particular purpose. The Maintainer is not
obliged to accept or use any Contribution.

## 8. Changes, and ending this agreement

You agree to tell the Maintainer if you learn that any promise in section 5 has become inaccurate.
You may end this agreement for future Contributions by telling the Maintainer in writing; the
licences already granted for Contributions submitted before then stay in force.

## How to sign

**Individuals** sign through a bot. On your first pull request, a check named `cla` asks you to
sign; reply to the pull request with exactly this sentence, from the account that made it:

```text
I have read the Nazm Contributor Licence Agreement, version 0.1, and I agree to it.
```

The bot records your GitHub account, the pull request and the time, and the check then passes on
this and every later pull request. The record is kept in the Nazm repository
(`cla/signatures.json` on the `cla-signatures` branch; `.github/workflows/cla.yml`).

**An entity** signing for its employees writes to the Maintainer, naming itself, the person
signing for it and the people whose Contributions it covers, and tells the Maintainer when that list
changes; each of those people then also signs through the bot. The Maintainer keeps a record of
every signature.

A new version of this agreement is announced in the repository, and a contributor signs it before
their next Contribution under it.
