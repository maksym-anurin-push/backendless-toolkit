---
name: Cycle path errors
overview: "Покращити повідомлення про циклічні залежності в tables_dependency_order: знаходити конкретні цикли в залишку після Kahn і друкувати кожен як окремий рядок шляху A -> B -> A."
todos: []
isProject: false
---

# Цикли в помилці `show_tables` як шляхи

## Очікуваний stderr

Замість:

```text
dependency cycle involving: RequestMoney, RequestMoneyReceivers, ...
```

щось на кшталт:

```text
dependency cycle:
RequestMoney -> RequestMoneyReceivers -> RequestMoney
SupplyChainBuyer -> SupplyChainSupplier -> SupplyChainInvoice -> SupplyChainBuyer
```

- один цикл = один рядок;
- стрілка ASCII `->` (як у прикладі користувача);
- шлях замкнений (останній вузол = перший);
- self-ref уніфікувати: `Tree -> Tree` у тому ж форматі (заголовок + рядок).

## Реалізація в [`backendless_toolkit_bin/src/db.rs`](backendless_toolkit_bin/src/db.rs)

1. Під час побудови графа зберегти також **forward** adjacency: `from → {to}` (зараз є лише reverse + indegree). Self-ref лишається окремим early-return, але текст через спільний форматер.

2. Після невдалого Kahn зібрати `residual` = вузли з `indegree > 0`. Серед них можуть бути й «хвости», що лише залежать від циклу — тому шукати **реальні** elementary cycles, а не просто сортувати residual.

3. **`fn find_dependency_cycles(forward, residual) -> Vec<Vec<String>>`**
   - DFS по residual з recursion stack;
   - при back-edge зібрати цикл `stack[i..] + start`;
   - канонізувати: ротація так, щоб лексикографічно найменше ім’я було першим (детермінізм);
   - дедуп однакових канонічних циклів;
   - сортувати список циклів за рядком шляху.

4. **`fn format_dependency_cycles(cycles: &[Vec<String>]) -> String`**
   - `dependency cycle:\n` + рядки `a -> b -> ... -> a`.

5. У `tables_dependency_order` при `ordered.len() != names.len()` викликати find + format замість `involving: ...`.

6. Оновити тести:
   - `A↔B` → містить `A -> B -> A` (або `B -> A -> B` після канонізації — з `A` першим буде `A -> B -> A`);
   - self-ref → `Tree -> Tree`;
   - два роз’єднані цикли → два рядки;
   - вузол поза циклом, що залежить від циклу (`D → A↔B`) → у повідомленні лише цикл `A/B`, без `D` як окремого «циклу».

SDK / CLI wiring не чіпати.