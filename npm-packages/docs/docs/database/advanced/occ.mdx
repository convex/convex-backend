---
title: "OCC and Atomicity"
slug: "occ"
hidden: false
sidebar_position: 500
todo: Push under mutations, or inline, or kill (move to Stack)
description:
  "Optimistic concurrency control and transaction atomicity in Convex"
---

In [Queries](/functions/query-functions.mdx), we mentioned that determinism was
important in the way optimistic concurrency control (OCC) was used within
Convex. In this section, we'll dive much deeper into _why_.

## Convex Financial, Inc.

Imagine that you're building a banking app, and therefore your databases stores
accounts with balances. You want your users to be able to give each other money,
so you write a mutation function that transfers funds from one user's account to
another.

One run of that transaction might read Alice's account balance, and then Bob's.
You then propose to deduct $5 from Alice's account and increase Bob's balance by
the same $5.

Here's our pseudocode:

```
$14 <- READ Alice
$11 <- READ Bob
WRITE Alice $9
WRITE Bob $16
```

This ledger balance transfer is a classic database scenario that requires a
guarantee that these write operations will only apply together. It is a really
bad thing if only one operation succeeds!

```
$14 <- READ Alice
$11 <- READ Bob
WRITE Alice $9
*crash* // $5 lost from your bank
```

You need a guarantee that this can never happen. You require transaction
atomicity, and Convex provides it.

The problem of data correctness is much deeper. Concurrent transactions that
read and edit the same records can create _data races_.

In the case of our app it's entirely possible that someone deducts Alice's
balance right after we read it. Maybe she bought a Coke Zero at the airport with
her debit card for $3.

```
$5 Transfer                           $3 Debit Card Charge
----------------------------------------------------------
$14 <- READ Alice
$11 <- READ Bob
                                        $14 <- READ Alice
                                        WRITE Alice $11
WRITE Alice $9 // Free coke!
WRITE Bob $16
```

Clearly, we need to prevent these types of data races from happening. We need a
way to handle these concurrent conflicts. Generally, there are two common
approaches.

Most traditional databases choose a _pessimistic locking_ strategy. (Pessimism
in this case means the strategy assumes conflict will happen ahead of time so
seeks to prevent it.) With pessimistic locking, you first need to acquire a lock
on Alice's record, and then acquire a lock on Bob's record. Then you can proceed
to conduct your transaction, knowing that any other transaction that needed to
touch those records will wait until you are done and all your writes are
committed.

After decades of experience, the drawbacks of pessimistic locking are well
understood and undeniable. The biggest limitation arises from real-life networks
and computers being inherently unreliable. If the lock holder goes missing for
whatever reason half way through its transaction, everyone else that wants to
modify any of those records is waiting indefinitely. Not good!

Optimistic concurrency control is, as the name states, optimistic. It assumes
the transaction will succeed and doesn't worry about locking anything ahead of
time. Very brash! How can it be so sure?

It does this by treating the transaction as a _declarative proposal_ to write
records on the basis of any read record versions (the "read set"). At the end of
the transaction, the writes all commit if every version in the read set is still
the latest version of that record. This means no concurrent conflict occurred.

Now using our version read set, let's see how OCC would have prevented the
soda-catastrophe above:

```
$5 Transfer                           $3 Debit Card Charge
----------------------------------------------------------
(v1, $14) <- READ Alice
(v7, $11) <- READ Bob
                                        (v1, $14) <- READ Alice
                                        WRITE Alice $11
                                        IF Alice.v = v1

WRITE Alice = $9, Bob = $16
    IF Alice.v = v1, Bob.v = v7 // Fails! Alice is = v2
```

This is akin to being unable to push your Git repository because you're not at
HEAD. We all know in that circumstance, we need to pull, and rebase or merge,
etc.

## When OCC loses, determinism wins

A naive optimistic concurrency control solution would be to solve this the same
way that Git does: require the user/application to resolve the conflict and
determine if it is safe to retry.

In Convex, however, we don't need to do that. We know the transaction is
deterministic. It didn't charge money to Stripe, it didn't write a permanent
value out to the filesystem. It had no effect at all other than proposing some
atomic changes to Convex tables that were not applied.

The determinism means that we can simply re-run the transaction; you never need
to worry about temporary data races. We can run several retries if necessary
until we succeed to execute the transaction without any conflicts.

<Admonition type="tip">

In fact, the Git analogy stays very apt. An OCC conflict means we cannot push
because our HEAD is out of date, so we need to rebase our changes and try again.
And determinism is what guarantees there is never a "merge conflict", so (unlike
with Git) this rebase operation will always eventually succeed without developer
intervention.

</Admonition>

## Snapshot Isolation vs Serializability

It is common for optimistic multi-version concurrency control databases to
provide a guarantee of
[snapshot isolation](https://en.wikipedia.org/wiki/Snapshot_isolation). This
[isolation level](<https://en.wikipedia.org/wiki/Isolation_(database_systems)>)
provides the illusion that all transactions execute on an atomic snapshot of the
data but it is vulnerable to
[anomalies](https://en.wikipedia.org/wiki/Snapshot_isolation#Definition) where
certain combinations of concurrent transactions can yield incorrect results. The
implementation of optimistic concurrency control in Convex instead provides true
[serializability](https://en.wikipedia.org/wiki/Serializability) and will yield
correct results regardless of what transactions are issued concurrently.

## No need to think about this

The beauty of this approach is that you can simply write your mutation functions
as if they will _always succeed_, and always be guaranteed to be atomic.

Aside from sheer curiosity about how Convex works, day to day there's no need to
worry about conflicts, locking, or atomicity when you make changes to your
tables and documents. The "obvious way" to write your mutation functions will
just work.
