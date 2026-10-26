import {test,expect} from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
const routes=['/','/projects','/projects/view?slug=payments-api','/jobs','/jobs/view?id=API-42','/runs','/runs/view?id=run-103','/approvals','/approvals/view?id=ap-42','/jobs/review?id=API-42','/analytics','/providers','/resources','/settings','/license','/artifacts/view?id=a1'];
for(const [index,route] of routes.entries())test(`${route} renders in dark and light and passes axe`,async({page})=>{
 await page.goto(route);await expect(page.locator('h1')).toBeVisible();
 const report=await new AxeBuilder({page}).withTags(['wcag2a','wcag2aa','wcag21aa','wcag22aa']).analyze();
 expect(report.violations.filter(v=>['critical','serious'].includes(v.impact??'')),JSON.stringify(report.violations)).toEqual([]);
 await page.screenshot({path:`screenshots/${String(index+1).padStart(2,'0')}-dark.png`,fullPage:true});
 await page.locator('[data-theme-toggle]').click();await expect(page.locator('html')).toHaveAttribute('data-theme','light');
 await page.screenshot({path:`screenshots/${String(index+1).padStart(2,'0')}-light.png`,fullPage:true});
});
test('unknown IDs have a useful state',async({page})=>{await page.goto('/jobs/view?id=API-999');await expect(page.getByRole('alert')).toContainText('API-999')});
test('command palette opens by keyboard',async({page})=>{await page.goto('/');await page.keyboard.press('Control+k');await expect(page.getByRole('dialog',{name:'Command palette'})).toBeVisible()});
test('known query ID resolves without build-time route',async({page})=>{await page.goto('/jobs/view?id=API-44');await expect(page.getByRole('heading',{name:/API-44/})).toBeVisible();await expect(page.getByRole('heading',{name:'Add per-client rate limiting'})).toBeHidden()});
test('project to job detail keeps the mock workflow context',async({page})=>{await page.goto('/projects/view?slug=payments-api');await expect(page.getByRole('heading',{name:'Payments API'})).toBeVisible();await page.getByRole('link',{name:/API-42/}).first().click();await expect(page).toHaveURL(/jobs\/view\?id=API-42/);await expect(page.getByRole('heading',{name:'Add per-client rate limiting'})).toBeVisible();await expect(page.getByRole('heading',{name:'Workflow'})).toBeVisible();await expect(page.getByRole('heading',{name:'Tasks'})).toBeVisible()});
test('approval queue opens the bound review document',async({page})=>{await page.goto('/approvals');await page.getByRole('link',{name:'Review →'}).first().click();await expect(page).toHaveURL(/approvals\/view\?id=ap-42/);await expect(page.getByRole('heading',{name:'What you are approving'})).toBeVisible();await expect(page.locator('pre')).toContainText('acceptance');await expect(page.getByRole('button',{name:/Approve .* A/})).toBeVisible()});
test('theme persists without first-paint flash',async({page})=>{await page.goto('/');await page.locator('[data-theme-toggle]').click();await page.reload();await expect(page.locator('html')).toHaveAttribute('data-theme','light')});
test('mobile approval decisions are read-only',async({page})=>{await page.setViewportSize({width:390,height:844});await page.goto('/approvals/view?id=ap-42');await expect(page.getByText('Mobile approvals are read-only')).toBeVisible();await expect(page.getByRole('button',{name:/Approve .* A/})).toBeHidden()});
for(const [name,route] of [['overview','/'],['job','/jobs/view?id=API-42'],['approval','/approvals/view?id=ap-42']] as const){
 test(`${name} stays accessible on mobile`,async({page})=>{await page.setViewportSize({width:390,height:844});await page.goto(route);await expect(page.locator('h1')).toBeVisible();const report=await new AxeBuilder({page}).withTags(['wcag2a','wcag2aa','wcag21aa','wcag22aa']).analyze();expect(report.violations.filter(v=>['critical','serious'].includes(v.impact??'')),JSON.stringify(report.violations)).toEqual([]);await page.screenshot({path:`screenshots/mobile-${name}.png`,fullPage:true})});
}
test('scope quick approval confirms the bound hash',async({page})=>{await page.goto('/');await page.getByRole('button',{name:'Quick approve scope'}).click();await expect(page.getByRole('dialog',{name:'Confirm scope approval'})).toContainText('sha256:');await page.getByRole('button',{name:'Confirm approval'}).click();await expect(page.getByRole('button',{name:'Approved'})).toBeVisible()});
