use formlogic_core::engine::FormLogicEngine;

fn probe(label: &str, code: &str, expected: &str) {
    let engine = FormLogicEngine::default();
    match engine.eval(code) {
        Ok(obj) => {
            let got = obj.inspect();
            if got == expected {
                println!("PASS: {label}");
            } else {
                println!("FAIL: {label} — expected {expected}, got {got}");
            }
        }
        Err(e) => println!("ERR:  {label} — {e}"),
    }
}

fn main() {
    // Recursive instance method (was crashing)
    probe("recursive instance method", r#"
        class Node {
            constructor(val, next) { this.val = val; this.next = next; }
            sum() { return this.val + (this.next ? this.next.sum() : 0); }
        }
        let list = new Node(1, new Node(2, new Node(3, null)));
        list.sum()
    "#, "6");

    // Generator return value
    probe("generator return", r#"
        function* g() { yield 1; return 2; yield 3; }
        let it = g();
        let a = it.next().value;
        let b = it.next().value;
        let c = it.next().done;
        a + "," + b + "," + c
    "#, "1,2,true");

    // Map chained set
    probe("Map chained set", r#" let m = new Map().set("a",1).set("b",2); m.get("b") "#, "2");

    // Spread Set/Map
    probe("Set spread", r#" let s = new Set([1,2,3]); [...s].join(",") "#, "1,2,3");
    probe("Map spread", r#" let m = new Map([["a",1]]); [...m].map(e => e[0] + e[1]).join(",") "#, "a1");

    // Edge cases
    probe("empty array join", r#" [].join(",") "#, "");
    probe("single elem join", r#" [42].join(",") "#, "42");
    probe("nested empty flat", r#" [[],[]].flat().length "#, "0");
    probe("null to string", r#" String(null) "#, "null");
    probe("undefined to string", r#" String(undefined) "#, "undefined");
    probe("bool to string", r#" String(true) "#, "true");
    probe("number to string", r#" String(42) "#, "42");
    probe("empty obj keys", r#" Object.keys({}).length "#, "0");

    // Complex patterns
    probe("immediately destructured", r#" let {a, b} = (() => ({a: 1, b: 2}))(); a + b "#, "3");
    probe("rest in object destr", r#" let {a, ...rest} = {a:1, b:2, c:3}; Object.keys(rest).join(",") "#, "b,c");
    probe("computed access", r#" let o = {x: 42}; let k = "x"; o[k] "#, "42");
    probe("bracket assign", r#" let o = {}; o["x"] = 42; o.x "#, "42");
    probe("nested template", r#" let a = "world"; `hello ${`dear ${a}`}` "#, "hello dear world");
    probe("chained ternary", r#"
        function classify(n) {
            return n > 100 ? "big" : n > 10 ? "medium" : "small";
        }
        classify(5) + "," + classify(50) + "," + classify(500)
    "#, "small,medium,big");

    // Split with regex
    probe("split regex", r#" "a1b2c".split(/\d/).join(",") "#, "a,b,c");

    // String concat method
    probe("str concat", r#" "hello".concat(" ", "world") "#, "hello world");

    // Generator manual iteration
    probe("gen manual iter", r#" function* g() { yield 1; yield 2; } let a = []; let it = g(); let n = it.next(); while(!n.done) { a.push(n.value); n = it.next(); } a.join(",") "#, "1,2");

    // Spread into function
    probe("spread into fn", r#" function sum(a,b,c) { return a+b+c; } sum(...[1,2,3]) "#, "6");

    // Class getter inherited from super
    probe("class getter super", r#"
        class Base {
            constructor() { this._v = 10; }
            get v() { return this._v; }
        }
        class Child extends Base { constructor() { super(); } }
        new Child().v
    "#, "10");

    // Nested class in function
    probe("class in function", r#"
        function createClass() {
            class Inner { constructor(x) { this.x = x; } value() { return this.x; } }
            return new Inner(42);
        }
        createClass().value()
    "#, "42");

    // Builder pattern
    probe("builder pattern", r#"
        class Builder {
            constructor() { this.items = []; }
            add(x) { this.items.push(x); return this; }
            build() { return this.items.join(","); }
        }
        new Builder().add("a").add("b").add("c").build()
    "#, "a,b,c");

    // Closure capturing multiple vars
    probe("closure multi capture", r#"
        function make(x, y) { return () => x + y; }
        make(10, 20)()
    "#, "30");

    // Array.from with length object
    probe("Array.from length obj", r#" Array.from({length: 3}, (_, i) => i).join(",") "#, "0,1,2");

    // typeof on async
    probe("async typeof", r#" async function f() { return 42; } typeof f() "#, "object");

    // Regex groups
    probe("regex capture groups", r#" "2024-01-15".match(/(\d{4})-(\d{2})-(\d{2})/)[1] "#, "2024");

    // Nested class inheritance
    probe("3 level inheritance", r#"
        class A { constructor() { this.x = 1; } }
        class B extends A { constructor() { super(); this.y = 2; } }
        class C extends B { constructor() { super(); this.z = 3; } }
        let c = new C();
        c.x + c.y + c.z
    "#, "6");

    // String.prototype methods
    probe("str.concat method", r#" "hello".concat(" ", "world") "#, "hello world");
    probe("str.match null", r#" "abc".match(/xyz/) "#, "null");

    // Map clear
    probe("Map clear", r#" let m = new Map([["a",1]]); m.clear(); m.size "#, "0");

    // Set delete
    probe("Set delete", r#" let s = new Set([1,2,3]); s.delete(2); s.size "#, "2");

    // Set clear
    probe("Set clear", r#" let s = new Set([1,2,3]); s.clear(); s.size "#, "0");

    // Chained array operations
    probe("complex chain", r#"
        [5,3,8,1,9,2,7,4,6]
            .filter(x => x > 3)
            .sort((a,b) => a - b)
            .map(x => x * 2)
            .join(",")
    "#, "10,12,14,16,18");

    // Nested function scoping
    probe("nested function scope", r#"
        function outer() {
            let x = 10;
            function inner() { return x + 5; }
            return inner();
        }
        outer()
    "#, "15");

    // Error in try-catch with finally
    probe("try catch finally error", r#"
        let log = [];
        try {
            log.push("try");
            throw new Error("oops");
        } catch(e) {
            log.push("catch:" + e.message);
        } finally {
            log.push("finally");
        }
        log.join(",")
    "#, "try,catch:oops,finally");

    // RegExp test with global flag
    probe("regex global test", r#"
        let re = /a/g;
        let count = 0;
        let m;
        while((m = re.exec("banana")) !== null) count++;
        count
    "#, "3");

    println!("\nDone.");
}
