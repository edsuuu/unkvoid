@props(['rows'])
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="margin:18px 0 6px;border:1px solid #241f34;border-radius:12px">
    @foreach ($rows as $label => $value)
        <tr>
            <td style="padding:10px 14px;border-bottom:1px solid #241f34;font-family:'IBM Plex Mono',Menlo,Consolas,monospace;font-size:10.5px;letter-spacing:0.12em;text-transform:uppercase;color:#8a80a6;white-space:nowrap">{{ $label }}</td>
            <td style="padding:10px 14px;border-bottom:1px solid #241f34;font-size:13.5px;color:#ece9f3">{{ $value }}</td>
        </tr>
    @endforeach
</table>
