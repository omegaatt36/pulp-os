//! Execute the shipped browser script against controlled DOM, XHR, and timers.
use std::process::Command;

const PAGE: &str = include_str!("../../assets/upload.html");
const HARNESS: &str = r#"
const assert = require('node:assert/strict');
const vm = require('node:vm');
const token = '0123456789abcdef0123456789abcdef';
const nextToken = 'abcdef0123456789abcdef0123456789';
const requests = [];
const elements = {};
let now = 0, nextTimer = 1;
const timers = new Map();
function setTimeout(callback, delay) {
    const id = nextTimer++;
    timers.set(id, {callback, due:now + delay, delay});
    return id;
}
function clearTimeout(id) { timers.delete(id); }
function advance(ms) {
    const end = now + ms;
    for (let steps = 0; ; steps++) {
        assert.ok(steps < 100, 'timer loop must terminate');
        const next = [...timers].filter(([,t])=>t.due <= end).sort((a,b)=>a[1].due-b[1].due)[0];
        if (!next) break;
        now = next[1].due;
        timers.delete(next[0]);
        next[1].callback();
    }
    now = end;
}
function createElement() {
    let html = '';
    return {textContent:'', className:'', value:'', hidden:false, style:{}, dataset:{}, children:[],
        classList:{add(){}, remove(){}}, parent:null,
        get innerHTML(){return html},
        set innerHTML(value){html=value; this.children.forEach(c=>c.parent=null); this.children=[]},
        appendChild(child){child.parent=this; this.children.push(child)},
        remove(){if(this.parent){this.parent.children=this.parent.children.filter(c=>c!==this); this.parent=null}},
        click(){}};
}
function element(id) {return elements[id] ||= createElement();}
function XHR() {this.headers={}; this.upload={}; this.finished=false; this.abortCount=0; requests.push(this);}
XHR.prototype.open = function(method,path) {this.method=method; this.path=path;};
XHR.prototype.setRequestHeader = function(name,value) {this.headers[name]=value;};
XHR.prototype.send = function(body) {
    assert.equal(requests.filter(r=>r.sent && !r.finished).length, 0,
        'the single-connection server cannot accept overlapping XHRs: ' + this.path);
    this.body=body; this.sent=true;
    assert.ok(this.timeout > 0, this.path + ' needs a deadline');
    this.deadline=setTimeout(()=>complete(this,'timeout'),this.timeout);
};
XHR.prototype.abort = function() {
    this.abortCount++;
    if (this.sent && !this.finished) complete(this,'abort');
};
function complete(request, kind='load', status=200, body='[]') {
    assert.ok(request.sent && !request.finished, 'complete an unfinished sent request');
    request.finished=true;
    clearTimeout(request.deadline);
    request.status=status;
    request.responseText=body;
    request['on'+kind]();
}
function inflight(path) {
    const active=requests.filter(r=>r.sent && !r.finished);
    assert.equal(active.length, 1, 'exactly one active request expected');
    if(path) assert.equal(active[0].path,path);
    return active[0];
}
function handoff() {noRequests(); advance(99); noRequests(); advance(1);}
function respond(path, status=200, body='[]') {
    complete(inflight(path),'load',status,body);
    handoff();
}
function authorize(value) {context.authorize(value); handoff();}
function paths() {return requests.map(r=>r.path);}
function noRequests() {assert.equal(requests.filter(r=>r.sent && !r.finished).length,0);}
function file(name) {return {name, size:10};}
function snapshot() {
    return JSON.stringify(['status','time-status','empty','auth','fill'].map(id=>{
        const e=element(id); return [e.textContent,e.className,e.hidden,e.style.width];
    }));
}
const scenario=process.env.SCENARIO;
const fragment=(scenario==='missing' || scenario==='manual') ? '' : '#' + token;
const context={document:{getElementById:element, createElement},
    window:{location:{hash:fragment}}, XMLHttpRequest:XHR,
    FormData:function(){this.fields=[]; this.append=(name,value)=>this.fields.push([name,value]);},
    confirm:()=>true, setTimeout, clearTimeout, Date, JSON, console};
vm.createContext(context);
vm.runInContext(process.env.UPLOAD_SCRIPT,context);
context.window.onload();
if(fragment) handoff();
function idle() {respond('/time'); respond('/files'); noRequests();}
function late(request) {
    request.status=403;
    request.onload(); request.onerror(); request.ontimeout(); request.onabort();
    if(request.upload.onprogress) request.upload.onprogress({lengthComputable:true,loaded:1,total:10});
}
if(scenario==='missing') {
    context.load(); context.calibrateTime(); context.upload([file('test.txt')]); context.del('test.txt',createElement());
    assert.equal(requests.length,0);
    assert.equal(element('auth').hidden,false);
} else if(scenario==='manual' || scenario==='fragment') {
    if(scenario==='manual') {
        assert.equal(requests.length,0);
        element('token').value='invalid';
        element('auth-form').onsubmit({preventDefault(){}});
        assert.equal(requests.length,0);
        element('token').value=token.slice(0,16).toUpperCase()+' '+token.slice(16);
        element('auth-form').onsubmit({preventDefault(){}});
        handoff();
    }
    const time=inflight('/time');
    assert.match(time.body,/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}$/);
    assert.equal(time.timeout,5000);
    respond('/time');
    context.upload([file('test.txt')]); context.del('test.txt',createElement());
    assert.deepEqual(paths(),['/time','/files']);
    respond('/files'); respond('/upload'); respond('/delete'); respond('/files');
    assert.deepEqual(paths(),['/time','/files','/upload','/delete','/files']);
    for(const request of requests) {
        assert.equal(request.headers['X-Pulp-Token'],token,request.path);
        assert.ok(!request.path.includes(token));
    }
} else if(scenario.startsWith('rejected_')) {
    const endpoint=scenario.split('_')[1];
    if(endpoint!=='time') respond('/time');
    if(endpoint==='upload' || endpoint==='delete') respond('/files');
    if(endpoint==='delete') context.del('old.txt',createElement());
    context.upload([file('a.txt'),file('b.txt')]);
    context.del('queued.txt',createElement()); context.load();
    respond('/'+endpoint,403);
    const count=requests.length;
    assert.equal(element('auth').hidden,false);
    assert.equal(context.busy,0);
    context.load(); context.upload([file('blocked.txt')]); context.del('blocked.txt',createElement());
    advance(1000000);
    assert.equal(requests.length,count);
    authorize(nextToken); respond('/time'); respond('/files');
    assert.deepEqual(paths().slice(count),['/time','/files']);
    assert.equal(requests[count].headers['X-Pulp-Token'],nextToken);
} else if(scenario==='batch_two' || scenario==='batch_three') {
    const files=[file('a.txt'),file('b.txt')];
    if(scenario==='batch_three') files.push(file('c.txt'));
    context.upload(files);
    context.load(); context.load();
    respond('/time');
    assert.deepEqual(paths(),['/time','/upload']);
    respond('/upload');
    const second=inflight('/upload');
    second.upload.onprogress({lengthComputable:true,loaded:40,total:100});
    advance(1000);
    assert.equal(element('fill').style.width,'40%','old success timers must not reset the next upload');
    context.del('delete.txt',createElement());
    context.upload([file('later.txt')]);
    context.load(); context.load();
    while(inflight().path==='/upload') respond('/upload');
    respond('/delete'); respond('/upload');
    assert.equal(requests.filter(r=>r.path==='/files').length,1);
    respond('/files'); noRequests();
    advance(1000); noRequests();
    assert.equal(element('fill').style.width,'100%');
} else if(scenario==='queued_listing') {
    idle();
    context.upload([file('first.txt')]);
    context.del('delete.txt',createElement());
    respond('/upload');
    inflight('/delete');
    context.upload([file('later.txt')]);
    respond('/delete'); respond('/upload'); respond('/files');
    assert.equal(requests.filter(r=>r.path==='/files').length,2);
    noRequests();
} else if(scenario==='time_abort') {
    context.upload([file('after.txt')]);
    inflight('/time').abort();
    handoff();
    assert.equal(element('time-status').className,'err');
    respond('/upload'); respond('/files'); noRequests();
} else if(scenario==='listing_retry') {
    respond('/time');
    for(let i=0;i<5;i++) context.load();
    context.del('queued.txt',createElement());
    advance(5000);
    handoff();
    inflight('/delete');
    for(let i=0;i<5;i++) context.load();
    context.upload([file('queued.txt')]);
    advance(400);
    respond('/delete'); respond('/upload');
    assert.equal(requests.filter(r=>r.path==='/files').length,2);
    respond('/files'); noRequests();
    advance(10000); noRequests();
} else if(scenario==='retry_exhaustion') {
    respond('/time');
    for(const delay of [400,800,1600]) {
        complete(inflight('/files'),'error');
        context.load(); context.load();
        advance(delay-1); noRequests();
        advance(1); inflight('/files');
    }
    complete(inflight('/files'),'error');
    assert.equal(element('empty').textContent,'Connection error');
    assert.equal(requests.filter(r=>r.path==='/files').length,4);
    advance(10000); noRequests();
} else if(scenario==='timeout_abort') {
    context.upload([file('timeout.txt'),file('abort.txt')]);
    context.del('delete.txt',createElement());
    advance(5000);
    handoff();
    assert.equal(element('time-status').className,'err');
    assert.equal(inflight('/upload').timeout,610000);
    advance(610000);
    handoff();
    const second=inflight('/upload');
    assert.equal(element('fill').style.width,'0');
    second.upload.onprogress({lengthComputable:true,loaded:50,total:100});
    second.abort();
    handoff();
    assert.equal(element('fill').style.width,'0');
    assert.equal(inflight('/delete').timeout,35000);
    advance(35000);
    handoff();
    assert.match(element('status').textContent,/Delete failed: request timed out/);
    inflight('/files').abort();
    advance(1000000); noRequests();
    assert.deepEqual(paths(),['/time','/upload','/upload','/delete','/files']);
    assert.equal(context.busy,0);
} else if(scenario==='ordinary_failures') {
    idle();
    context.upload([file('server.txt'),file('network.txt'),file('good.txt')]);
    respond('/upload',500,'write failed');
    complete(inflight('/upload'),'error');
    handoff();
    const last=inflight('/upload');
    last.upload.onprogress({lengthComputable:true,loaded:100,total:100});
    assert.match(element('status').textContent,/Saving good.txt on device/);
    assert.equal(requests.filter(r=>r.path==='/files').length,1);
    respond('/upload'); respond('/files');
    context.del('abort.txt',createElement());
    context.upload([file('after-delete.txt')]);
    inflight('/delete').abort();
    handoff();
    respond('/upload'); respond('/files');
    noRequests();
    assert.equal(requests.filter(r=>r.path==='/upload').length,4);
} else if(scenario==='session_replacement') {
    idle();
    context.upload([file('old.txt'),file('discard.txt')]);
    context.del('discard.txt',createElement());
    const old=inflight('/upload');
    old.upload.onprogress({lengthComputable:true,loaded:50,total:100});
    authorize(nextToken);
    assert.equal(old.abortCount,1);
    const fresh=inflight('/time'), count=requests.length, ui=snapshot();
    late(old); advance(1000);
    assert.equal(snapshot(),ui);
    assert.equal(requests.length,count);
    assert.equal(context.sessionToken,nextToken);
    assert.equal(inflight(),fresh);
    respond('/time'); respond('/files');
    context.upload([file('fresh.txt')]);
    const upload=inflight('/upload');
    upload.upload.onprogress({lengthComputable:true,loaded:75,total:100});
    const freshUI=snapshot();
    late(old);
    assert.equal(snapshot(),freshUI);
    assert.equal(context.busy,1);
    respond('/upload'); respond('/files'); noRequests();
} else if(scenario==='stale_retry') {
    respond('/time');
    const old=inflight('/files');
    complete(old,'error');
    const retry=[...timers.values()].find(t=>t.delay===400).callback;
    const handoffCallback=[...timers.values()].find(t=>t.delay===100).callback;
    context.authorize(nextToken);
    const count=requests.length, ui=snapshot();
    retry(); handoffCallback(); late(old);
    assert.equal(snapshot(),ui);
    assert.equal(requests.length,count);
    handoff();
    advance(400);
    assert.equal(requests.length,count+1);
    assert.equal(snapshot(),ui);
    respond('/time'); respond('/files'); noRequests();
} else if(scenario==='delete_after_refresh') {
    respond('/time');
    respond('/files',200,'[{"name":"old.txt","size":10}]');
    const oldRow=element('list').children[0];
    context.upload([file('new.txt')]);
    respond('/upload');
    inflight('/files');
    oldRow.children[2].children[0].onclick();
    respond('/files',200,'[{"name":"old.txt","size":10},{"name":"new.txt","size":10}]');
    assert.equal(oldRow.parent,null,'the refresh replaces the clicked row');
    assert.equal(element('list').children.length,2);
    respond('/delete');
    assert.deepEqual(element('list').children.map(row=>row.children[0].textContent),['new.txt']);
    assert.deepEqual(paths(),['/time','/files','/upload','/files','/delete']);
    noRequests();
} else if(scenario==='malformed_listing') {
    respond('/time');
    for(const response of ['{','null','{}','[{"name":"bad.txt"}]','[null]']) {
        context.del('next.txt',createElement());
        respond('/files',200,response);
        assert.equal(element('empty').textContent,'Bad response');
        respond('/delete');
        context.load();
    }
    respond('/files',200,'[{"name":"good.txt","size":123}]');
    assert.equal(element('list').children.length,1);
    const row=element('list').children[0];
    row.children[2].children[0].onclick();
    respond('/delete');
    assert.equal(element('list').children.length,0);
    noRequests();
} else if(scenario==='reentrant_authorization') {
    idle();
    context.upload([file('old.txt'),file('discard.txt')]);
    const old=inflight('/upload'), original=context.s;
    context.s=function(message,cls) {
        original(message,cls);
        if(message.startsWith('old.txt uploaded')) context.authorize(nextToken);
    };
    respond('/upload');
    const fresh=inflight('/time');
    late(old);
    assert.equal(inflight(),fresh);
    respond('/time'); respond('/files'); noRequests();
    assert.equal(context.sessionToken,nextToken);
    assert.equal(context.busy,0);
} else if(scenario==='start_reauthorization') {
    idle();
    const original=context.s;
    context.s=function(message,cls) {
        original(message,cls);
        if(message==='Uploading old.txt...') context.authorize(nextToken);
    };
    context.upload([file('old.txt'),file('discard.txt')]);
    const abandoned=requests[requests.length-1];
    assert.equal(abandoned.path,'/upload');
    assert.equal(abandoned.sent,undefined);
    assert.equal(abandoned.abortCount,1);
    handoff(); respond('/time'); respond('/files'); noRequests();
    assert.equal(context.sessionToken,nextToken);
} else throw new Error('unknown scenario '+scenario);
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
        "{scenario}: {}",
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
fn rejected_token_clears_work_and_allows_correction() {
    for endpoint in ["time", "files", "upload", "delete"] {
        browser(&format!("rejected_{endpoint}"));
    }
}
#[test]
fn upload_batches_and_later_additions_refresh_once() {
    browser("batch_two");
    browser("batch_three");
    browser("queued_listing");
}
#[test]
fn listing_retries_coalesce_and_wait_for_mutations() {
    browser("listing_retry");
    browser("retry_exhaustion");
}
#[test]
fn request_deadlines_and_aborts_release_the_queue() {
    browser("timeout_abort");
    browser("time_abort");
}
#[test]
fn failed_mutations_continue_without_retry() {
    browser("ordinary_failures");
}
#[test]
fn replacement_session_ignores_late_events_and_progress() {
    browser("session_replacement");
}
#[test]
fn replacement_session_cancels_and_ignores_old_retries() {
    browser("stale_retry");
}
#[test]
fn malformed_listing_releases_the_queue() {
    browser("malformed_listing");
}
#[test]
fn callback_reauthorization_preserves_new_queue() {
    browser("reentrant_authorization");
    browser("start_reauthorization");
}

#[test]
fn queued_delete_removes_the_row_replaced_by_listing_refresh() {
    browser("delete_after_refresh");
}
