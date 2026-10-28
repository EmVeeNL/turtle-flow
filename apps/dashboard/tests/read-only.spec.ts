import {expect, test} from '@playwright/test';

test.skip(process.env.PUBLIC_API_ADAPTER !== 'real', 'requires the real dashboard adapter');

const overview = {
  projects:[],
  jobs:[{id:'j1',key:'API-42',project_id:'p1',title:'Phone layout job',description:'Read this job from a phone.',type:'feature',profile:'standard',state:'scope_approval',priority:'normal',stage_index:2,provider:null,usage:{total:null,provenance:'unavailable'},estimate:null,elapsed:null,version:1}],
  approvals:[{id:'ap-1',job_id:'j1',gate_kind:'scope',subject_hash:'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',status:'pending',requested_at:'2026-10-25T10:00:00Z',binding:{artifacts:[{kind:'specification',rev:1,sha256:'sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210',content:'{"summary":"Phone review","criteria":[{"name":"read-only"}]}',id:'r1'}],base_commit:null,head_commit:null,diff_sha256:null,target_branch:null,budget_envelope:null,policy_version:1},flags:[],invalidated_if:[]}],
  runs:[],artifacts:[],tasks:[],resources:[],events:[],providers:[]
};

test.beforeEach(async({page})=>{
  await page.setViewportSize({width:390,height:844});
  await page.route('**/api/v1/events/stream**',route=>route.abort());
  await page.route('**/api/v1/me',route=>route.fulfill({json:{readonly:true}}));
  await page.route('**/api/v1/overview',route=>route.fulfill({json:overview}));
  await page.route('**/api/v1/approvals/ap-1',route=>route.fulfill({json:overview.approvals[0]}));
});

const pages: Array<[string,string]> = [
  ['overview','/'],['jobs','/jobs'],['job','/jobs/view?id=API-42'],['approval','/approvals/view?id=ap-1']
];
for (const [name,path] of pages) {
  test(`${name} read-only view fits a 390px phone`,async({page})=>{
    await page.goto(path);
    await expect(page.getByText(/This device is read-only/)).toBeVisible();
    await expect.poll(()=>page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth)).toBe(true);
    if(name==='job') await expect(page.getByRole('button',{name:/^(start|pause|resume|cancel)$/})).toHaveCount(0);
    if(name==='approval') {
      await expect(page.getByText(/sha256:0123456789abcdef/)).toBeVisible();
      await expect(page.getByRole('button',{name:/^(approve|changes|reject)$/})).toHaveCount(0);
      await expect(page.locator('pre')).toContainText('  "summary"');
    }
  });
}
