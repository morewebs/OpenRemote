import React, {useEffect} from 'react'
import {createRoot} from 'react-dom/client'
import {WebConsoleProvider} from '../../src/web/console.jsx'
import {useConsole} from '../../src/state/console.jsx'
window.peerRequests=[]
const peer={
  ensureDevice:async()=>({id:'fixture-browser',name:'Fixture browser'}),
  fetchFor:machine=>async(url,init)=>{
    const body=init.body?JSON.parse(init.body):null
    window.peerRequests.push({machine,url,body})
    if(url.endsWith('/sessions')&&init.method==='POST')return Response.json({id:'web-chat',harness:'claude',status:'idle',executor:machine,workspace:'/fixture'})
    if(url.endsWith('/capabilities'))return Response.json({harnesses:[]})
    if(url.endsWith('/projects'))return Response.json({projects:[]})
    if(window.rejectPrompt && /\/prompts$/.test(url))return Response.json({error:'Fixture delivery failed'},{status:503})
    return Response.json({ok:true,sessions:[],events:[]})
  },
}
function Expose(){const state=useConsole();useEffect(()=>{window.realWebConsole=state},[state]);return <p>Real web provider verification</p>}
createRoot(document.getElementById('root')).render(<WebConsoleProvider peer={peer} target={{api:null,issuer:'fixture'}} tokens={{access:null}} initialMachineId="fixture-machine"><Expose/></WebConsoleProvider>)
