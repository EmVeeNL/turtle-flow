export interface DiffFile { path:string; additions:number; deletions:number; protected?:boolean; lines:{kind:'context'|'add'|'remove'|'hunk';old?:number;next?:number;text:string}[]; }
export const diffFiles: DiffFile[] = [
 {path:'crates/api/src/rate_limit.rs',additions:108,deletions:0,lines:[
  {kind:'hunk',text:'@@ -0,0 +1,108 @@'},
  {kind:'add',next:1,text:'use governor::{Quota, RateLimiter};'},
  {kind:'add',next:2,text:'use std::num::NonZeroU32;'},
  {kind:'add',next:3,text:''},
  {kind:'add',next:4,text:'pub struct ClientRateLimit {'},
  {kind:'add',next:5,text:'    pub tier: ClientTier,'},
  {kind:'add',next:6,text:'    pub requests_per_second: NonZeroU32,'},
  {kind:'add',next:7,text:'}'},
  {kind:'add',next:8,text:''},
  {kind:'add',next:9,text:'impl ClientRateLimit {'},
  {kind:'add',next:10,text:'    pub fn check(&self, key: &str) -> Result<(), RateLimitError> {'},
  {kind:'add',next:11,text:'        self.limiter.check_key(key).map_err(|_| RateLimitError::Exceeded)'},
  {kind:'add',next:12,text:'    }'},
  {kind:'add',next:13,text:'}'},
 ]},
 {path:'crates/api/src/middleware.rs',additions:65,deletions:1,lines:[{kind:'hunk',text:'@@ -34,6 +34,9 @@'},{kind:'context',old:34,next:34,text:'pub async fn handle(req: Request) -> Response {'},{kind:'remove',old:35,text:'    next.run(req).await'},{kind:'add',next:35,text:'    let tier = resolve_client_tier(&req);'},{kind:'add',next:36,text:'    limiter.check(tier, api_key(&req))?;'},{kind:'add',next:37,text:'    next.run(req).await'}]},
 {path:'crates/api/tests/rate_limit.rs',additions:84,deletions:0,lines:[{kind:'hunk',text:'@@ -0,0 +1,84 @@'},{kind:'add',next:1,text:'#[test] fn over_limit_returns_429() {'},{kind:'add',next:2,text:'    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);'},{kind:'add',next:3,text:'}'}]},
 {path:'Cargo.lock',additions:38,deletions:0,protected:true,lines:[{kind:'hunk',text:'@@ -1821,6 +1821,11 @@'},{kind:'add',next:1821,text:'name = "governor"'},{kind:'add',next:1822,text:'version = "0.6.3"'}]}
];
