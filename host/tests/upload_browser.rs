//! Execute the shipped browser script against a minimal DOM and XHR probe.
use std::process::Command;

const PAGE: &str = include_str!("../../assets/upload.html");
const HARNESS: &str = r#"
const assert = require('node:assert/strict');
const vm = require('node:vm');
const token = '0123456789abcdef0123456789abcdef';
const requests = [];
const elements = {};
function element(id) {
    return elements[id] ||= {textContent:'', className:'', value:'', hidden:false,
        innerHTML:'', style:{}, children:[], classList:{add(){}, remove(){}},
        appendChild(child){this.children.push(child)}, remove(){}, click(){}};
}
function XHR() { this.headers = {}; this.upload = {}; requests.push(this); }
XHR.prototype.open = function(method,path) {this.method=method; this.path=path;};
XHR.prototype.setRequestHeader = function(name,value) {this.headers[name]=value;};
XHR.prototype.send = function(body) {this.body=body; this.sent=true;};
const fragment = (process.env.SCENARIO === 'fragment' || process.env.SCENARIO.startsWith('rejected')) ? '#' + token : '';
const context = {document:{getElementById:element, createElement:element},
    window:{location:{hash:fragment, pathname:'/', search:''}},
    location:{hash:fragment}, XMLHttpRequest:XHR, FormData:function(){this.append=()=>{}},
    fin:element('fin'), confirm:()=>true, setTimeout:()=>{}, Date, JSON,
    history:{replaceState(){}}, console};
vm.createContext(context);
vm.runInContext(process.env.UPLOAD_SCRIPT, context);
context.window.onload();
if (process.env.SCENARIO.startsWith('rejected')) {
    const endpoint = process.env.SCENARIO.split('_')[1] || 'time';
    if (endpoint !== 'time') {
        requests[0].status=200;
        requests[0].onload();
        if (endpoint === 'upload') context.upload([{name:'test.txt'}]);
        if (endpoint === 'delete') context.del('TEST.TXT', element('row'));
    }
    const rejectedRequest = requests[requests.length-1];
    assert.equal(rejectedRequest.path, '/' + endpoint);
    const count = requests.length;
    rejectedRequest.status=403;
    rejectedRequest.onload();
    assert.equal(element('auth').hidden, false, 'rejected tokens must allow correction');
    context.load();
    context.upload([{name:'test.txt'}]);
    context.del('TEST.TXT', element('row'));
    assert.equal(requests.length, count, 'rejection must stop subsequent requests');
    element('token').value = token;
    element('auth-form').onsubmit({preventDefault(){}});
    assert.equal(requests.length, count + 1);
    assert.equal(requests[count].path, '/time');
    requests[count].status=200;
    requests[count].onload();
    assert.equal(requests[count+1].path, '/files');
    assert.equal(requests[count+1].headers['X-Pulp-Token'], token);
} else if (process.env.SCENARIO === 'missing') {
    context.load();
    context.calibrateTime();
    context.upload([{name:'test.txt'}]);
    context.del('TEST.TXT', element('row'));
    assert.equal(requests.length, 0, 'a missing token must prevent every endpoint request');
    assert.equal(element('auth').hidden, false, 'manual authorization must be visible');
} else {
    if (process.env.SCENARIO === 'manual') {
        assert.equal(requests.length, 0);
        element('token').value = 'invalid';
        element('auth-form').onsubmit({preventDefault(){}});
        assert.equal(requests.length, 0, 'invalid manual input must not send requests');
        element('token').value = token.slice(0,16) + ' ' + token.slice(16);
        element('auth-form').onsubmit({preventDefault(){}});
    }
    assert.equal(requests.length, 1);
    assert.equal(requests[0].path, '/time');
    assert.match(requests[0].body, /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}$/);
    requests[0].status=200;
    requests[0].onload();
    context.upload([{name:'test.txt'}]);
    context.del('TEST.TXT', element('row'));
    assert.deepEqual(requests.map(r=>r.path), ['/time','/files','/upload','/delete']);
    for (const request of requests) {
        assert.equal(request.headers['X-Pulp-Token'], token, request.path);
        assert.ok(!request.path.includes(token), 'token must not enter endpoint URLs');
    }
}
"#;

fn browser(scenario: &str) {
    let script = PAGE
        .split("<script>")
        .nth(1)
        .unwrap()
        .split("</script>")
        .next()
        .unwrap();
    let output = Command::new("node")
        .args(["-e", HARNESS])
        .env("UPLOAD_SCRIPT", script)
        .env("SCENARIO", scenario)
        .output()
        .expect("Node.js is required to execute the shipped upload client");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn missing_token_sends_no_endpoint_requests() {
    browser("missing");
}
#[test]
fn fragment_token_authenticates_every_endpoint() {
    browser("fragment");
}
#[test]
fn manual_token_authenticates_every_endpoint() {
    browser("manual");
}

#[test]
fn rejected_token_can_be_corrected_without_reload() {
    for endpoint in ["time", "files", "upload", "delete"] {
        browser(&format!("rejected_{endpoint}"));
    }
}
