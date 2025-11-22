# midnight-scavenger-miner

**This software is provided “as-is” without any warranties. The author is not responsible for any loss, damage, or consequences resulting from its use**

`midnight-scavenger-miner` is a personal tool to participate in midnight scavenger mining

The goal is to provide a **distributed, stateless, and easily scalable** mining setup

To run the full system, you will need:

- One mongoDB, submitter and workers must be able to reach this
- One submitter service
- Multiple workers

## How it works (Stateless & Horizontally Scalable)
```

                     +----------------------+
                     |   Midnight Endpoint  |
                     +----------+-----------+
                                ^
                                |
                                |
                        +---------------+
                        |   Submitter   |
                        |  (fetch tasks |
                        |   & submit)   |
                        +-------+-------+
                                |
                                v
                       +-----------------+
                       |     MongoDB     |
                       | (state & unique |
                       |   task claims)  |
                       +--------+--------+
                                |
     -------------------------------------------------
     |                 |                |            |
     v                 v                v            v
+------------+   +------------+   +------------+  +------------+
|  Worker 1  |   |  Worker 2  |   |  Worker 3  |  |  Worker n  |
| (fetch +   |   | (fetch +   |   | (fetch +   |  | (fetch +   |
|  claim +   |   |  claim +   |   |  claim +   |  |  claim +   |
|  solve)    |   |  solve)    |   |  solve)    |  |  solve)    |
+------------+   +------------+   +------------+  +------------+
```

The entire design is built around being **stateless**, so workers can scale horizontally without coordination.  
All state lives in **MongoDB**, and **unique indexes** ensure that no two workers ever duplicate work.


1. **Task discovery**  
   The worker fetches all active challenges and addresses from MongoDB, then creates a list of tasks.  
   Each task is a unique `challenge:address` pair.

2. **Claiming a task using MongoDB unique index**  
   Before doing any computation, the worker attempts to insert a *solution placeholder* into MongoDB with  
   `_id = "challenge:address"`.

   MongoDB enforces a **unique index** on this field.  
   - If the insert **succeeds**, this worker has claimed the task.  
   - If it **fails**, another worker is already processing it (or a solution already exists), so the task is skipped.

   This mechanism removes the need for locks, coordination, or leader election.  
   It makes scaling trivial: just start more workers.

3. **Solving the task**  
   Once the task is claimed, the worker computes the solution.  
   When done, it updates the placeholder document and marks it as `"solved"`.

4. **Submitting solutions**  
   A separate **submitter service** periodically scans MongoDB for documents with status `"solved"`.  
   It then submits those solutions to the Midnight network.

Because the workers store no state locally and rely solely on MongoDB for coordination, you can run as many workers as you want, on any machine with Docker, without worrying about race conditions or duplicate work. Just run:

```bash
docker run <image> <instance_id>